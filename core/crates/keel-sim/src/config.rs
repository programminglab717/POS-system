//! What a run is made of: every parameter drawn from its seed, so a seed is the whole run.

use crate::rng::Rng;

/// A run's parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    /// The seed everything else in the run is drawn from.
    pub seed: u64,
    /// How many devices ring orders: 2 or 3.
    pub devices: u8,
    /// How long, in virtual milliseconds, devices work and faults strike, before everything
    /// heals.
    pub working: i64,
    /// How long after healing the replicas must agree, in virtual milliseconds.
    pub agree_within: i64,
    /// The shortest and longest a frame takes, in milliseconds.
    pub delay: (i64, i64),
    /// Frames lost while the network is faulty, per thousand.
    pub loss: u64,
    /// Frames duplicated while the network is faulty, per thousand.
    pub duplication: u64,
    /// Links cut for a while.
    pub partitions: u8,
    /// Nodes crashing, between writes or in the middle of one, and restarting.
    pub crashes: u8,
    /// Devices whose store is restored from an older copy of itself.
    pub rollbacks: u8,
    /// Clocks jumping forwards or back.
    pub clock_jumps: u8,
    /// The shortest and longest wait between a device's commands, in milliseconds.
    pub pace: (i64, i64),
}

impl Config {
    /// A run with faults, its parameters drawn from `seed`.
    pub fn from_seed(seed: u64) -> Config {
        let mut rng = Rng::new(seed ^ 0x5EED_C0DE);
        let faults = |rng: &mut Rng, most: u64| u8::try_from(rng.below(most)).unwrap_or(0);
        Config {
            seed,
            devices: if rng.chance(300) { 3 } else { 2 },
            working: 30_000,
            agree_within: 60_000,
            delay: (1, rng.between(2, 120)),
            loss: if rng.chance(250) { 0 } else { rng.below(250) },
            duplication: if rng.chance(250) { 0 } else { rng.below(150) },
            partitions: faults(&mut rng, 4),
            crashes: faults(&mut rng, 4),
            rollbacks: faults(&mut rng, 3),
            clock_jumps: faults(&mut rng, 4),
            pace: (100, rng.between(300, 1500)),
        }
    }

    /// A run without faults: no frame lost or duplicated, no partition, crash, rollback or clock
    /// jump. Every event should reach every replica quickly.
    pub fn calm(seed: u64) -> Config {
        Config {
            loss: 0,
            duplication: 0,
            partitions: 0,
            crashes: 0,
            rollbacks: 0,
            clock_jumps: 0,
            ..Config::from_seed(seed)
        }
    }
}
