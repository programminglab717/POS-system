//! # keel-net
//!
//! The local network ([ADR-0026](../../../../docs/adr/0026-local-hub.md), decision 3): `keel-sync`'s
//! frames between a location's devices and its Store Hub, over WebSocket on TLS 1.3 with mutual
//! TLS. It moves frames and decides nothing: the runtime's thread hands it the frames to send
//! and takes the frames that arrive, so the store keeps its one writer.
//!
//! - **The hub** ([`Net::hub`]) listens on one port; **a device** ([`Net::device`]) connects to
//!   the roster's address, trying each address the host resolves to, and again after a backoff
//!   from 1 s doubling to 30 s, jittered, when the connection drops.
//! - **The cloud** ([ADR-0028](../../../../docs/adr/0028-cloud-cell-v0.md)): a roster that names
//!   one has the hub dial it beside its listener, on the one network, as a device dials the hub;
//!   the cloud's certificate must name the roster's cloud device and carry the cloud's name. The
//!   cell listens for each location with [`Net::cell`], which admits the devices that may host
//!   the hub alone.
//! - **Identity**: each side presents its certificate and verifies the other's against the
//!   location's CA alone, with `rustls` and its `ring` provider, TLS 1.3 only. A device is the one
//!   whose name its certificate carries, `<uuid>.devices.keel` ([`device_name`]): exactly one
//!   such name, and no wildcard. It must be in the roster, or the connection closes; the hub's
//!   certificate also carries the roster's hub name, which a device checks instead of an
//!   address, so a new address doesn't break it, and must name a device the roster lets host
//!   the hub.
//! - **Frames**: each binary WebSocket message is one frame of at most `keel-sync`'s
//!   [`MAX_FRAME`] bytes; anything else closes the connection. Each side pings every 5 s, and a
//!   connection silent for 15 s, or whose writes stall for as long, closes.
//! - **Bounded**: every queue has a bound, and a frame that finds its queue full is dropped, as
//!   the replicator sends again what a peer lacks; at most 32 handshakes run at once.
//!
//! It runs its own `tokio` runtime on a thread of its own, and talks to the caller through
//! channels: [`Net::send`] and [`Net::recv_timeout`].

use core::time::Duration;
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpListener as StdListener};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;

use futures_util::{SinkExt, StreamExt};
use keel_domain::roster::Roster;
use keel_events::envelope::Device;
use keel_sync::MAX_FRAME;
use keel_types::Id;
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Semaphore, mpsc as tokio_mpsc, watch};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

/// What a device dials: where, the name the peer's certificate must carry, and which device it
/// must be.
struct Target {
    host: String,
    port: u16,
    name: String,
    peer: Peer,
}

/// Which device a dialed peer must be.
#[derive(Clone, Copy)]
enum Peer {
    /// Any the roster lets host the hub.
    Hub,
    /// This one.
    Device(Id<Device>),
    /// Any of the roster's but its cloud, which the hub dials itself: an inbound connection
    /// with the cloud's certificate would replace the one it dialed.
    NotCloud,
}

/// What every device's name ends with.
const DEVICES: &str = ".devices.keel";

/// The name a device's certificate carries: `<uuid>.devices.keel`.
pub fn device_name(device: Id<Device>) -> String {
    format!("{device}{DEVICES}")
}

/// What a device presents, and trusts.
pub struct Credentials {
    /// The location's CA, the only one trusted.
    pub ca: CertificateDer<'static>,
    /// The device's certificate, and any issuing it below the CA.
    pub chain: Vec<CertificateDer<'static>>,
    /// The device's TLS key.
    pub key: PrivateKeyDer<'static>,
}

impl Clone for Credentials {
    fn clone(&self) -> Credentials {
        Credentials { ca: self.ca.clone(), chain: self.chain.clone(), key: self.key.clone_key() }
    }
}

/// What happens on the network.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A peer connected, its identity proved.
    Connected(Id<Device>),
    /// A frame from a peer.
    Frame {
        /// The peer.
        from: Id<Device>,
        /// The frame.
        frame: Vec<u8>,
    },
    /// A peer's connection ended.
    Disconnected(Id<Device>),
}

/// Why the network couldn't start.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum NetError {
    /// The credentials or TLS settings were refused.
    #[error("TLS: {0}")]
    Tls(#[from] rustls::Error),
    /// The CA's certificate couldn't be trusted.
    #[error("the CA's certificate: {0}")]
    Ca(String),
    /// A socket or the runtime couldn't be set up.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The roster's hub name, or its cloud's, isn't a DNS name a certificate can carry.
    #[error("the peer's name isn't a DNS name")]
    HubName,
    /// The roster names no cloud to dial.
    #[error("the roster names no cloud")]
    NoCloud,
}

/// The longest a device waits before connecting again.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// How long a TLS and WebSocket handshake may take.
const HANDSHAKE: Duration = Duration::from_secs(10);

/// How often each side of a connection pings the other.
const PING: Duration = Duration::from_secs(5);

/// How long a connection may go without a message, or a write may stall, before it closes.
const SILENCE: Duration = Duration::from_secs(15);

/// How many handshakes the hub runs at once; a connection beyond them is closed at once.
const MAX_HANDSHAKES: usize = 32;

/// How many frames wait for one connection's writer; a frame beyond them is dropped.
const MAX_QUEUED: usize = 256;

/// How many events, and frames to route, wait for the caller; one beyond them is dropped.
const MAX_EVENTS: usize = 1024;

/// The connections open now, each with the channel to its writer, and a number telling a newer
/// connection of a peer from the one it replaced.
type Connections = Arc<Mutex<BTreeMap<Id<Device>, (u64, tokio_mpsc::Sender<Vec<u8>>)>>>;

/// A device's or the hub's network.
pub struct Net {
    outgoing: tokio_mpsc::Sender<(Id<Device>, Vec<u8>)>,
    events: mpsc::Receiver<Event>,
    local: Option<SocketAddr>,
    shutdown: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

fn roots(ca: &CertificateDer<'static>) -> Result<Arc<RootCertStore>, NetError> {
    let mut roots = RootCertStore::empty();
    roots.add(ca.clone()).map_err(|error| NetError::Ca(error.to_string()))?;
    Ok(Arc::new(roots))
}

fn websocket() -> WebSocketConfig {
    let most = MAX_FRAME.saturating_add(64);
    WebSocketConfig::default().max_message_size(Some(most)).max_frame_size(Some(most))
}

/// The roster's device that `certificate` names: the one device's name it carries, with no
/// other device's name, and no wildcard that could stand for one.
fn identify(certificate: &CertificateDer<'_>, roster: &Roster) -> Option<Id<Device>> {
    let parsed = webpki::EndEntityCert::try_from(certificate).ok()?;
    let mut names = parsed
        .valid_dns_names()
        .filter(|name| name.contains('*') || name.to_ascii_lowercase().ends_with(DEVICES));
    let name = names.next()?;
    if names.next().is_some() || name.contains('*') {
        return None;
    }
    let device = Id::parse(name.strip_suffix(DEVICES)?).ok()?;
    if device_name(device) != name {
        return None;
    }
    roster.device(device)?;
    let subject = ServerName::try_from(name).ok()?;
    parsed.verify_is_valid_for_subject_name(&subject).ok()?;
    Some(device)
}

/// The 64-bit FNV-1a hash of `bytes`.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A number from 0 up to `most`, the next of a sequence `seed` carries.
fn jitter(seed: &mut u64, most: u64) -> u64 {
    // Knuth's MMIX constants: an LCG is enough to spread reconnections apart.
    *seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
    (*seed >> 33).checked_rem(most.saturating_add(1)).unwrap_or(0)
}

/// How long to wait before connecting again: from half of `backoff` up to all of it.
fn waited(backoff: Duration, seed: &mut u64) -> Duration {
    let millis = u64::try_from(backoff.as_millis()).unwrap_or(u64::MAX);
    let half = millis / 2;
    Duration::from_millis(half.saturating_add(jitter(seed, millis.saturating_sub(half))))
}

/// The server's half of TLS: `credentials`, verifying each client against the CA alone.
fn acceptor(credentials: Credentials) -> Result<TlsAcceptor, NetError> {
    let verifier = WebPkiClientVerifier::builder_with_provider(roots(&credentials.ca)?, provider())
        .build()
        .map_err(|error| NetError::Ca(error.to_string()))?;
    let config = ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_client_cert_verifier(verifier)
        .with_single_cert(credentials.chain, credentials.key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

/// The client's half of TLS: `credentials`, verifying each server against the CA alone.
fn connector(credentials: Credentials) -> Result<TlsConnector, NetError> {
    let config = ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])?
        .with_root_certificates(roots(&credentials.ca)?)
        .with_client_auth_cert(credentials.chain, credentials.key)?;
    Ok(TlsConnector::from(Arc::new(config)))
}

/// Whether `device`, which the roster holds, is the peer `peer` asks for.
fn admits(roster: &Roster, peer: Peer, device: Id<Device>) -> bool {
    match peer {
        // A device the roster lets host the hub.
        Peer::Hub => roster.device(device).is_some_and(|device| device.hub.is_some()),
        Peer::Device(expected) => device == expected,
        Peer::NotCloud => roster.cloud().is_none_or(|cloud| cloud.device != device),
    }
}

/// What a listener takes: the devices it admits, as they prove who they are.
async fn accept(
    listener: StdListener,
    acceptor: TlsAcceptor,
    roster: Arc<Roster>,
    peer: Peer,
    shared: Shared,
) {
    let Ok(listener) = TcpListener::from_std(listener) else { return };
    let handshakes = Arc::new(Semaphore::new(MAX_HANDSHAKES));
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            // Out of descriptors, say: wait, rather than spin.
            tokio::time::sleep(Duration::from_millis(100)).await;
            continue;
        };
        // Too many handshakes at once: this one closes, and the device tries again.
        let Ok(permit) = Arc::clone(&handshakes).try_acquire_owned() else { continue };
        let (acceptor, roster, shared) = (acceptor.clone(), Arc::clone(&roster), shared.clone());
        tokio::task::spawn_local(async move {
            let accepted = tokio::time::timeout(HANDSHAKE, async {
                stream.set_nodelay(true).ok()?;
                let tls = acceptor.accept(stream).await.ok()?;
                let peer_certificate = tls.get_ref().1.peer_certificates()?.first()?.clone();
                let device = identify(&peer_certificate, &roster)?;
                if !admits(&roster, peer, device) {
                    return None;
                }
                let ws = tokio_tungstenite::accept_async_with_config(tls, Some(websocket()))
                    .await
                    .ok()?;
                Some((device, ws))
            })
            .await;
            drop(permit);
            if let Ok(Some((device, ws))) = accepted {
                shared.serve(device, ws).await;
            }
        });
    }
}

/// A peer to dial: where, and how.
struct Dialer {
    target: Target,
    connector: TlsConnector,
    server_name: ServerName<'static>,
    /// Each device's own sequence, drawn from its certificate, so devices that lose a peer
    /// together come back apart.
    seed: u64,
}

impl Dialer {
    fn new(credentials: Credentials, target: Target) -> Result<Dialer, NetError> {
        // A DNS name, not an address: the name is what stays when the address changes.
        let Ok(server_name @ ServerName::DnsName(_)) = ServerName::try_from(target.name.clone())
        else {
            return Err(NetError::HubName);
        };
        let seed = credentials.chain.first().map_or(0, |certificate| fnv(certificate));
        Ok(Dialer { connector: connector(credentials)?, server_name, seed, target })
    }

    /// Dials the target, and again after a backoff whenever the connection drops.
    async fn run(mut self, roster: Arc<Roster>, shared: Shared) {
        let url = format!("wss://{}/sync", self.target.name);
        let mut backoff = Duration::from_secs(1);
        loop {
            let connected = tokio::time::timeout(HANDSHAKE, async {
                let stream = connect(&self.target.host, self.target.port).await?;
                stream.set_nodelay(true).ok()?;
                let tls = self.connector.connect(self.server_name.clone(), stream).await.ok()?;
                let peer = tls.get_ref().1.peer_certificates()?.first()?.clone();
                let device = identify(&peer, &roster)?;
                if !admits(&roster, self.target.peer, device) {
                    return None;
                }
                let (ws, _) = tokio_tungstenite::client_async_with_config(
                    url.as_str(),
                    tls,
                    Some(websocket()),
                )
                .await
                .ok()?;
                Some((device, ws))
            })
            .await;
            if let Ok(Some((device, ws))) = connected {
                backoff = Duration::from_secs(1);
                shared.clone().serve(device, ws).await;
            }
            tokio::time::sleep(waited(backoff, &mut self.seed)).await;
            backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);
        }
    }
}

/// The cloud `roster` names, to dial.
fn cloud_target(roster: &Roster) -> Option<Target> {
    roster.cloud().map(|cloud| Target {
        host: cloud.host.clone(),
        port: cloud.port.get(),
        name: cloud.name.clone(),
        peer: Peer::Device(cloud.device),
    })
}

impl Net {
    /// The hub's network, listening on `listener` for the devices of `roster`, and dialing the
    /// cloud the roster names, if it names one (ADR-0028): the hub carries its location's log
    /// to the cloud, which can't dial a hub behind the store's router.
    ///
    /// # Errors
    /// [`NetError`] if the credentials aren't usable, the cloud's name isn't a DNS name, or the
    /// runtime can't start.
    pub fn hub(
        listener: StdListener,
        roster: Roster,
        credentials: Credentials,
    ) -> Result<Net, NetError> {
        let dialer = cloud_target(&roster)
            .map(|target| Dialer::new(credentials.clone(), target))
            .transpose()?;
        let acceptor = acceptor(credentials)?;
        Net::listen(listener, roster, acceptor, Peer::NotCloud, dialer)
    }

    /// The cloud's network for one location (ADR-0028), listening on `listener` for the devices
    /// of `roster` that may host its hub, which dial it.
    ///
    /// # Errors
    /// [`NetError`] if the credentials aren't usable, or the runtime can't start.
    pub fn cell(
        listener: StdListener,
        roster: Roster,
        credentials: Credentials,
    ) -> Result<Net, NetError> {
        let acceptor = acceptor(credentials)?;
        Net::listen(listener, roster, acceptor, Peer::Hub, None)
    }

    /// A network listening on `listener` for the devices `peer` admits, and dialing `dialer`.
    fn listen(
        listener: StdListener,
        roster: Roster,
        acceptor: TlsAcceptor,
        peer: Peer,
        dialer: Option<Dialer>,
    ) -> Result<Net, NetError> {
        listener.set_nonblocking(true)?;
        let local = listener.local_addr()?;
        Net::spawn(Some(local), move |shared| async move {
            let roster = Arc::new(roster);
            if let Some(dialer) = dialer {
                tokio::task::spawn_local(dialer.run(Arc::clone(&roster), shared.clone()));
            }
            accept(listener, acceptor, roster, peer, shared).await;
        })
    }

    /// A device's network, connecting to the hub `roster` names, and again whenever the
    /// connection drops.
    ///
    /// # Errors
    /// [`NetError`] if the credentials aren't usable, the hub's name isn't a DNS name, or the
    /// runtime can't start.
    pub fn device(roster: Roster, credentials: Credentials) -> Result<Net, NetError> {
        let hub = &roster.data().hub;
        let target = Target {
            host: hub.host.clone(),
            port: hub.port.get(),
            name: hub.name.clone(),
            peer: Peer::Hub,
        };
        Net::dial(roster, credentials, target)
    }

    /// A device's network to its location's cloud alone (ADR-0028): connecting to the cloud
    /// `roster` names, whose certificate must name the roster's cloud device and carry the
    /// cloud's name, and again whenever the connection drops. [`Net::hub`] dials it beside its
    /// listener.
    ///
    /// # Errors
    /// [`NetError::NoCloud`] if the roster names no cloud; otherwise as [`Net::device`].
    pub fn cloud(roster: Roster, credentials: Credentials) -> Result<Net, NetError> {
        let target = cloud_target(&roster).ok_or(NetError::NoCloud)?;
        Net::dial(roster, credentials, target)
    }

    /// A network dialing `target`, and again whenever the connection drops.
    fn dial(roster: Roster, credentials: Credentials, target: Target) -> Result<Net, NetError> {
        let dialer = Dialer::new(credentials, target)?;
        Net::spawn(None, move |shared| dialer.run(Arc::new(roster), shared))
    }

    /// Starts the runtime's thread, running `main` and the router of outgoing frames until the
    /// network is dropped.
    fn spawn<F, Fut>(local: Option<SocketAddr>, main: F) -> Result<Net, NetError>
    where
        F: FnOnce(Shared) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_io().enable_time().build()?;
        let (outgoing, mut routed) = tokio_mpsc::channel::<(Id<Device>, Vec<u8>)>(MAX_EVENTS);
        let (events_tx, events) = mpsc::sync_channel(MAX_EVENTS);
        let (shutdown, mut stopped) = watch::channel(false);
        let shared =
            Shared { connections: Arc::default(), events: events_tx, next: Arc::default() };
        let thread = std::thread::Builder::new().name("keel-net".into()).spawn(move || {
            let local_set = tokio::task::LocalSet::new();
            local_set.block_on(&runtime, async move {
                let connections = Arc::clone(&shared.connections);
                tokio::task::spawn_local(main(shared));
                loop {
                    tokio::select! {
                        routed = routed.recv() => {
                            let Some((to, frame)) = routed else { return };
                            let connections = connections.lock().unwrap_or_else(PoisonError::into_inner);
                            // A frame for a peer not connected now, or whose writer is that far
                            // behind, is dropped: the replicator sends again what a peer lacks.
                            if let Some((_, writer)) = connections.get(&to) {
                                let _ = writer.try_send(frame);
                            }
                        }
                        _ = stopped.changed() => return,
                    }
                }
            });
        })?;
        Ok(Net { outgoing, events, local, shutdown, thread: Some(thread) })
    }

    /// Sends `frame` to `to`, if it is connected now and its queue has room.
    pub fn send(&self, to: Id<Device>, frame: Vec<u8>) {
        let _ = self.outgoing.try_send((to, frame));
    }

    /// The next thing that happens on the network, waiting at most `wait`.
    pub fn recv_timeout(&self, wait: Duration) -> Option<Event> {
        self.events.recv_timeout(wait).ok()
    }

    /// The address the hub listens on.
    pub const fn local_addr(&self) -> Option<SocketAddr> {
        self.local
    }
}

impl Drop for Net {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A connection to `host` and `port`, trying each address the host resolves to in turn.
async fn connect(host: &str, port: u16) -> Option<TcpStream> {
    for address in tokio::net::lookup_host((host, port)).await.ok()? {
        if let Ok(stream) = TcpStream::connect(address).await {
            return Some(stream);
        }
    }
    None
}

/// What every connection's task shares.
#[derive(Clone)]
struct Shared {
    connections: Connections,
    events: mpsc::SyncSender<Event>,
    next: Arc<Mutex<u64>>,
}

impl Shared {
    /// Tells the caller `event`, or drops it if the caller is that far behind: nothing here
    /// waits for the caller, so the network never stalls on it.
    fn tell(&self, event: Event) {
        let _ = self.events.try_send(event);
    }

    /// Carries frames between `device` and the caller over `ws` until either side ends it, or it
    /// falls silent. Reading and writing run side by side, so a peer that writes as much as it
    /// is sent never stalls the two.
    async fn serve<S>(self, device: Id<Device>, ws: tokio_tungstenite::WebSocketStream<S>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let (tx, mut rx) = tokio_mpsc::channel::<Vec<u8>>(MAX_QUEUED);
        let generation = {
            let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
            *next = next.saturating_add(1);
            *next
        };
        // A newer connection of the peer replaces an older one, whose writer then ends.
        self.connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(device, (generation, tx));
        self.tell(Event::Connected(device));
        let (mut sink, mut stream) = ws.split();
        let reader = async {
            loop {
                match tokio::time::timeout(SILENCE, stream.next()).await {
                    Ok(Some(Ok(Message::Binary(frame)))) if frame.len() <= MAX_FRAME => {
                        self.tell(Event::Frame { from: device, frame: frame.to_vec() });
                    }
                    // Pings are answered by the WebSocket itself.
                    Ok(Some(Ok(Message::Ping(_) | Message::Pong(_)))) => {}
                    _ => break,
                }
            }
        };
        let writer = async {
            let mut ping = tokio::time::interval(PING);
            ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                let message = tokio::select! {
                    frame = rx.recv() => match frame {
                        Some(frame) => Message::binary(frame),
                        None => break,
                    },
                    _ = ping.tick() => Message::Ping(Vec::new().into()),
                };
                if !matches!(tokio::time::timeout(SILENCE, sink.send(message)).await, Ok(Ok(()))) {
                    break;
                }
            }
        };
        tokio::select! {
            () = reader => {}
            () = writer => {}
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), sink.close()).await;
        let mut connections = self.connections.lock().unwrap_or_else(PoisonError::into_inner);
        if connections.get(&device).is_some_and(|(current, _)| *current == generation) {
            connections.remove(&device);
            drop(connections);
            self.tell(Event::Disconnected(device));
        }
    }
}
