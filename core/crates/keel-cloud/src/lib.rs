//! # keel-cloud
//!
//! The cloud cell v0 ([ADR-0028](../../../../docs/adr/0028-cloud-cell-v0.md)): for each location
//! it serves, the location's durable replica, a [`Location`]. Each is a device runtime over a
//! store of its own, opened as the location's cloud device with no one signed in, as the hub's
//! daemon opens its store, replicating with the location's hub through `keel-net`'s
//! [`Net::cell`], on a thread of its own. It verifies and folds every event as each device does,
//! so its totals are the kernel's.
//!
//! Anything else that reads or writes a location's store, such as the back office's API, hands
//! the location's thread a job ([`Location::with`]), which it runs between two turns of its
//! loop: the store keeps its one writer.

use core::time::Duration;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;

use keel_domain::profile::Profile;
use keel_domain::roster::Roster;
use keel_events::envelope::Location as LocationRef;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_net::Net;
use keel_provision::Kit;
use keel_runtime::{Identity, Runtime, RuntimeError};
use keel_store::{StoreError, StoreKey};
use keel_types::{Id, Locale, OsEntropy, SystemClock};

/// A location's durable replica's runtime: the cloud device's.
pub type CloudRuntime = Runtime<SoftwareSigner, OsEntropy, SystemClock, OsEntropy>;

/// The longest a location's loop waits for the network, so a job waits no longer to run.
pub const STEP_WAIT: Duration = Duration::from_millis(50);

/// Why a location couldn't be served.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CellError {
    /// The kit can't be read, or a file of it doesn't decode.
    #[error("the kit: {0}")]
    Kit(String),
    /// The kit isn't the cloud's of its location: its roster names no cloud, or another device.
    #[error("the kit isn't its location's cloud's")]
    NotCloud,
    /// The store or the runtime refused.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// The network couldn't start.
    #[error(transparent)]
    Net(#[from] keel_net::NetError),
    /// A socket or a thread couldn't be set up.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// The location's loop has stopped: its store refused everything until opened again.
    #[error("the location's replica has stopped: {0}")]
    Stopped(String),
}

/// What a location's thread runs between two turns of its loop.
type Job = Box<dyn FnOnce(&mut CloudRuntime) + Send>;

/// A location the cell serves: its durable replica, on a thread of its own.
pub struct Location {
    id: Id<LocationRef>,
    name: String,
    roster: Roster,
    jobs: Option<mpsc::Sender<Job>>,
    thread: Option<JoinHandle<Result<(), String>>>,
}

/// The parts of a cloud's kit the location's thread opens its runtime with.
struct Opened {
    kit: Kit,
    roster: Roster,
    profile: Profile,
}

/// The cloud's kit in `dir`, checked to be its location's cloud's.
fn read(dir: &Path) -> Result<Opened, CellError> {
    let kit = Kit::read(dir).map_err(|error| CellError::Kit(error.to_string()))?;
    let roster = Roster::decode(&kit.roster).map_err(|error| CellError::Kit(error.to_string()))?;
    let profile =
        Profile::decode(&kit.profile).map_err(|error| CellError::Kit(error.to_string()))?;
    if roster.cloud().is_none_or(|cloud| cloud.device != kit.device) {
        return Err(CellError::NotCloud);
    }
    Ok(Opened { kit, roster, profile })
}

fn open(opened: &Opened, store: PathBuf) -> Result<CloudRuntime, CellError> {
    let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &opened.kit.signing_key)
        .map_err(|error| CellError::Kit(error.to_string()))?;
    let mut store_key = opened.kit.store_key;
    Ok(Runtime::open(
        store,
        StoreKey::new(&mut store_key),
        Identity { device: opened.kit.device, signer },
        opened.profile.clone(),
        SystemClock,
        OsEntropy,
        OsEntropy,
        Locale::EN_US,
    )?)
}

impl Location {
    /// Serves the location whose cloud's kit is in `kit`, its store in `kit/store.db`, listening
    /// on `listener` for the devices that may host its hub.
    ///
    /// # Errors
    /// [`CellError`] if the kit isn't its location's cloud's, or the store, the runtime or the
    /// network can't be opened.
    pub fn open(kit: &Path, listener: TcpListener) -> Result<Location, CellError> {
        let opened = read(kit)?;
        let net = Net::cell(listener, opened.roster.clone(), opened.kit.credentials())?;
        let location = opened.roster.data().location;
        let (name, roster) = (opened.profile.data().name.to_string(), opened.roster.clone());
        let store = kit.join("store.db");
        let (started_tx, started) = mpsc::sync_channel(1);
        let (jobs, queue) = mpsc::channel::<Job>();
        let thread =
            std::thread::Builder::new().name(format!("keel-cell-{location}")).spawn(move || {
                let mut runtime = match open(&opened, store).and_then(|mut runtime| {
                    // The cloud names no durable peer and never hosts the hub: it is the
                    // durable copy.
                    let first = runtime.start_sync_as(opened.roster.clone(), false)?;
                    for frame in first {
                        net.send(frame.to, frame.frame);
                    }
                    Ok(runtime)
                }) {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let message = error.to_string();
                        let _ = started_tx.send(Err(error));
                        return Err(message);
                    }
                };
                let _ = started_tx.send(Ok(()));
                serve(&mut runtime, &net, &queue)
            })?;
        started.recv().map_err(|_| CellError::Stopped("it didn't start".into()))??;
        Ok(Location { id: location, name, roster, jobs: Some(jobs), thread: Some(thread) })
    }

    /// The location's identifier.
    pub const fn id(&self) -> Id<LocationRef> {
        self.id
    }

    /// The location's name, as its profile gives it.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The location's roster.
    pub const fn roster(&self) -> &Roster {
        &self.roster
    }

    /// Runs `job` on the location's thread, between two turns of its loop, and gives back what
    /// it answers.
    ///
    /// # Errors
    /// [`CellError::Stopped`] if the location's loop has stopped.
    pub fn with<R: Send + 'static>(
        &self,
        job: impl FnOnce(&mut CloudRuntime) -> R + Send + 'static,
    ) -> Result<R, CellError> {
        let (answer, answered) = mpsc::sync_channel(1);
        let stopped = || CellError::Stopped("its store refused everything".into());
        self.jobs
            .as_ref()
            .ok_or_else(stopped)?
            .send(Box::new(move |runtime| {
                let _ = answer.send(job(runtime));
            }))
            .map_err(|_| stopped())?;
        answered.recv().map_err(|_| stopped())
    }
}

impl Drop for Location {
    fn drop(&mut self) {
        // The loop ends when its queue does.
        drop(self.jobs.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The location's loop: each job waiting, then a turn of replication, until the queue ends or
/// the store refuses everything until it is opened again.
fn serve(runtime: &mut CloudRuntime, net: &Net, queue: &mpsc::Receiver<Job>) -> Result<(), String> {
    loop {
        loop {
            match queue.try_recv() {
                Ok(job) => job(runtime),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => return Ok(()),
            }
        }
        match keel_hub::step(runtime, net, STEP_WAIT) {
            Ok(_) => {}
            // A store that refuses everything until it is opened again: stop, rather than spin.
            Err(RuntimeError::Store(error @ (StoreError::Damaged | StoreError::Closed))) => {
                return Err(error.to_string());
            }
            // Anything else may pass: the peer sends it again.
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}
