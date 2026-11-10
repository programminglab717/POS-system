//! # keel-hub
//!
//! The Store Hub's daemon ([ADR-0026](../../../../docs/adr/0026-local-hub.md), decision 4): a
//! device runtime over its own store, no one signed in, replicating with the location's devices
//! through `keel-net`'s listener. And [`step`], the loop that joins any runtime to the network,
//! which the daemon, the shells and the tests run.

use core::time::Duration;

use keel_events::keys::Signer;
use keel_net::{Event, Net};
use keel_runtime::{Runtime, RuntimeError};
use keel_types::{Clock, Entropy};

/// The longest [`step`] waits for the network when the runtime asks for no tick sooner.
pub const MAX_WAIT: Duration = Duration::from_millis(500);

/// One turn of the loop that joins `runtime` to `net`: waits for a frame until the runtime's
/// next tick, at most `most`; takes in the frame; ticks the runtime when it asks; and sends what
/// the runtime answers, with what it wrote since it last pushed. Connections coming and going
/// change nothing: the replicator's rounds find out what each peer lacks. Answers whether it
/// took in something from the network, frame or not, so a caller can turn again at once while
/// more is waiting.
///
/// # Errors
/// [`RuntimeError`] if the store can't be read or written. Nothing a frame held was stored, and
/// the peer sends it again: the caller reports the error and goes on.
pub fn step<S: Signer, E: Entropy, C: Clock, I: Entropy>(
    runtime: &mut Runtime<S, E, C, I>,
    net: &Net,
    most: Duration,
) -> Result<bool, RuntimeError> {
    step_seeing(runtime, net, most, |_| {})
}

/// As [`step`], telling `seen` what the network brought first, a frame or a peer connecting or
/// leaving: for a caller that shows who is connected, such as the cell.
///
/// # Errors
/// As [`step`].
pub fn step_seeing<S: Signer, E: Entropy, C: Clock, I: Entropy>(
    runtime: &mut Runtime<S, E, C, I>,
    net: &Net,
    most: Duration,
    seen: impl FnOnce(&Event),
) -> Result<bool, RuntimeError> {
    let now = runtime.clock().now().as_millis();
    let wait = runtime.next_tick().map_or(most, |at| {
        let millis = u64::try_from(at.as_millis().saturating_sub(now)).unwrap_or(0);
        Duration::from_millis(millis).min(most)
    });
    // A tick asked for further ahead than any wait was asked for by a clock since set back: no
    // wait for it.
    let wait = if runtime.tick_due() { Duration::ZERO } else { wait };
    // Each part goes on whatever the one before it did, so a frame that fails never starves the
    // ticks; the first failure is the step's.
    let mut failed = None;
    let event = net.recv_timeout(wait);
    let took = event.is_some();
    if let Some(event) = &event {
        seen(event);
    }
    if let Some(Event::Frame { from, frame }) = event {
        match runtime.on_frame(from, &frame) {
            Ok(frames) => send(net, frames),
            Err(error) => failed = Some(error),
        }
    }
    if runtime.tick_due() {
        match runtime.on_tick() {
            Ok(frames) => send(net, frames),
            Err(error) => failed = failed.or(Some(error)),
        }
    }
    match runtime.take_frames() {
        Ok(frames) => send(net, frames),
        Err(error) => failed = failed.or(Some(error)),
    }
    failed.map_or(Ok(took), Err)
}

fn send(net: &Net, frames: Vec<keel_sync::Outgoing>) {
    for frame in frames {
        net.send(frame.to, frame.frame);
    }
}
