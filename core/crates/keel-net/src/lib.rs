//! # keel-net
//!
//! The local network ([ADR-0026](../../../../docs/adr/0026-local-hub.md), decision 3): `keel-sync`'s
//! frames between a location's devices and its Store Hub, over WebSocket on TLS 1.3 with mutual
//! TLS. It moves frames and decides nothing: the runtime's thread hands it the frames to send
//! and takes the frames that arrive, so the store keeps its one writer.
//!
//! - **The hub** ([`Net::hub`]) listens on one port; **a device** ([`Net::device`]) connects to
//!   the roster's address, and again after a backoff from 1 s doubling to 30 s when the connection
//!   drops.
//! - **Identity**: each side presents its certificate and verifies the other's against the
//!   location's CA alone, with `rustls` and its `ring` provider, TLS 1.3 only. A device is the one
//!   whose name its certificate carries, `<uuid>.devices.keel` ([`device_name`]), and must be in
//!   the roster, or the connection closes; the hub's certificate also carries the roster's hub
//!   name, which a device checks instead of an address, so a new address doesn't break it.
//! - **Frames**: each binary WebSocket message is one frame of at most `keel-sync`'s
//!   [`MAX_FRAME`] bytes; anything else closes the connection.
//!
//! It runs its own `tokio` runtime on a thread of its own, and talks to the caller through
//! channels: [`Net::send`] and [`Net::recv_timeout`].

use core::time::Duration;
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpListener as StdListener, ToSocketAddrs};
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
use tokio::sync::{mpsc as tokio_mpsc, watch};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;

/// The name a device's certificate carries: `<uuid>.devices.keel`.
pub fn device_name(device: Id<Device>) -> String {
    format!("{device}.devices.keel")
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
    /// The roster's hub name isn't a name a certificate can carry.
    #[error("the hub's name isn't a DNS name")]
    HubName,
}

/// The connections open now, each with the channel to its writer, and a number telling a newer
/// connection of a peer from the one it replaced.
type Connections = Arc<Mutex<BTreeMap<Id<Device>, (u64, tokio_mpsc::UnboundedSender<Vec<u8>>)>>>;

/// A device's or the hub's network.
pub struct Net {
    outgoing: tokio_mpsc::UnboundedSender<(Id<Device>, Vec<u8>)>,
    events: mpsc::Receiver<Event>,
    local: Option<SocketAddr>,
    shutdown: watch::Sender<bool>,
    thread: Option<JoinHandle<()>>,
}

/// The longest a device waits before connecting again.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// How long a TLS and WebSocket handshake may take.
const HANDSHAKE: Duration = Duration::from_secs(10);

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

/// The roster's device whose name `certificate` carries, if any.
fn identify(certificate: &CertificateDer<'_>, roster: &Roster) -> Option<Id<Device>> {
    let parsed = webpki::EndEntityCert::try_from(certificate).ok()?;
    roster.data().devices.iter().map(|device| device.id).find(|&device| {
        ServerName::try_from(device_name(device))
            .is_ok_and(|name| parsed.verify_is_valid_for_subject_name(&name).is_ok())
    })
}

impl Net {
    /// The hub's network, listening on `listener`, for the devices of `roster`.
    ///
    /// # Errors
    /// [`NetError`] if the credentials aren't usable, or the runtime can't start.
    pub fn hub(
        listener: StdListener,
        roster: Roster,
        credentials: Credentials,
    ) -> Result<Net, NetError> {
        let verifier =
            WebPkiClientVerifier::builder_with_provider(roots(&credentials.ca)?, provider())
                .build()
                .map_err(|error| NetError::Ca(error.to_string()))?;
        let config = ServerConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_client_cert_verifier(verifier)
            .with_single_cert(credentials.chain, credentials.key)?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        listener.set_nonblocking(true)?;
        let local = listener.local_addr()?;
        Net::spawn(Some(local), move |shared| async move {
            let Ok(listener) = TcpListener::from_std(listener) else { return };
            let roster = Arc::new(roster);
            loop {
                let Ok((stream, _)) = listener.accept().await else { continue };
                let (acceptor, roster, shared) =
                    (acceptor.clone(), Arc::clone(&roster), shared.clone());
                tokio::spawn(async move {
                    let accepted = tokio::time::timeout(HANDSHAKE, async {
                        let tls = acceptor.accept(stream).await.ok()?;
                        let peer = tls.get_ref().1.peer_certificates()?.first()?.clone();
                        let device = identify(&peer, &roster)?;
                        let ws =
                            tokio_tungstenite::accept_async_with_config(tls, Some(websocket()))
                                .await
                                .ok()?;
                        Some((device, ws))
                    })
                    .await;
                    if let Ok(Some((device, ws))) = accepted {
                        shared.serve(device, ws).await;
                    }
                });
            }
        })
    }

    /// A device's network, connecting to the hub `roster` names, and again whenever the
    /// connection drops.
    ///
    /// # Errors
    /// [`NetError`] if the credentials aren't usable, the hub's name isn't a DNS name, or the
    /// runtime can't start.
    pub fn device(roster: Roster, credentials: Credentials) -> Result<Net, NetError> {
        let config = ClientConfig::builder_with_provider(provider())
            .with_protocol_versions(&[&rustls::version::TLS13])?
            .with_root_certificates(roots(&credentials.ca)?)
            .with_client_auth_cert(credentials.chain, credentials.key)?;
        let connector = TlsConnector::from(Arc::new(config));
        let hub_name =
            ServerName::try_from(roster.data().hub.name.clone()).map_err(|_| NetError::HubName)?;
        Net::spawn(None, move |shared| async move {
            let hub = &roster.data().hub;
            let address = format!("{}:{}", hub.host, hub.port);
            let url = format!("wss://{}/sync", hub.name);
            let mut backoff = Duration::from_secs(1);
            loop {
                let target =
                    address.to_socket_addrs().ok().and_then(|mut addresses| addresses.next());
                let connected = tokio::time::timeout(HANDSHAKE, async {
                    let stream = TcpStream::connect(target?).await.ok()?;
                    stream.set_nodelay(true).ok()?;
                    let tls = connector.connect(hub_name.clone(), stream).await.ok()?;
                    let peer = tls.get_ref().1.peer_certificates()?.first()?.clone();
                    let device = identify(&peer, &roster)?;
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
                tokio::time::sleep(backoff).await;
                backoff = backoff.saturating_mul(2).min(MAX_BACKOFF);
            }
        })
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
        let (outgoing, mut routed) = tokio_mpsc::unbounded_channel::<(Id<Device>, Vec<u8>)>();
        let (events_tx, events) = mpsc::channel();
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
                            // A frame for a peer not connected now is dropped: the replicator
                            // sends again what a peer lacks.
                            if let Some((_, writer)) = connections.get(&to) {
                                let _ = writer.send(frame);
                            }
                        }
                        _ = stopped.changed() => return,
                    }
                }
            });
        })?;
        Ok(Net { outgoing, events, local, shutdown, thread: Some(thread) })
    }

    /// Sends `frame` to `to`, if it is connected now.
    pub fn send(&self, to: Id<Device>, frame: Vec<u8>) {
        let _ = self.outgoing.send((to, frame));
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

/// What every connection's task shares.
#[derive(Clone)]
struct Shared {
    connections: Connections,
    events: mpsc::Sender<Event>,
    next: Arc<Mutex<u64>>,
}

impl Shared {
    /// Carries frames between `device` and the caller over `ws` until either side ends it.
    async fn serve<S>(self, device: Id<Device>, ws: tokio_tungstenite::WebSocketStream<S>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    {
        let (tx, mut rx) = tokio_mpsc::unbounded_channel::<Vec<u8>>();
        let generation = {
            let mut next = self.next.lock().unwrap_or_else(PoisonError::into_inner);
            *next = next.saturating_add(1);
            *next
        };
        // A newer connection of the peer replaces an older one.
        self.connections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(device, (generation, tx));
        let _ = self.events.send(Event::Connected(device));
        let (mut sink, mut stream) = ws.split();
        loop {
            tokio::select! {
                frame = rx.recv() => {
                    let Some(frame) = frame else { break };
                    if sink.send(Message::binary(frame)).await.is_err() {
                        break;
                    }
                }
                message = stream.next() => match message {
                    Some(Ok(Message::Binary(frame))) if frame.len() <= MAX_FRAME => {
                        let _ = self.events.send(Event::Frame { from: device, frame: frame.to_vec() });
                    }
                    // Pings are answered by the WebSocket itself.
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                    _ => break,
                },
            }
        }
        let _ = sink.close().await;
        let mut connections = self.connections.lock().unwrap_or_else(PoisonError::into_inner);
        if connections.get(&device).is_some_and(|(current, _)| *current == generation) {
            connections.remove(&device);
            drop(connections);
            let _ = self.events.send(Event::Disconnected(device));
        }
    }
}
