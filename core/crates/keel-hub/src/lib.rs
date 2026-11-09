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
/// change nothing: the replicator's rounds find out what each peer lacks.
///
/// # Errors
/// [`RuntimeError`] if the store can't be read or written. Nothing a frame held was stored, and
/// the peer sends it again: the caller reports the error and goes on.
pub fn step<S: Signer, E: Entropy, C: Clock, I: Entropy>(
    runtime: &mut Runtime<S, E, C, I>,
    net: &Net,
    most: Duration,
) -> Result<(), RuntimeError> {
    let now = runtime.clock().now().as_millis();
    let wait = runtime.next_tick().map_or(most, |at| {
        let millis = u64::try_from(at.as_millis().saturating_sub(now)).unwrap_or(0);
        Duration::from_millis(millis).min(most)
    });
    if let Some(Event::Frame { from, frame }) = net.recv_timeout(wait) {
        send(net, runtime.on_frame(from, &frame)?);
    }
    if runtime.next_tick().is_some_and(|at| at <= runtime.clock().now()) {
        send(net, runtime.on_tick()?);
    }
    send(net, runtime.take_frames()?);
    Ok(())
}

fn send(net: &Net, frames: Vec<keel_sync::Outgoing>) {
    for frame in frames {
        net.send(frame.to, frame.frame);
    }
}
