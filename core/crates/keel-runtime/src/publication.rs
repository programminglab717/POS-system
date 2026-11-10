//! Catalog publishing in the runtime (ADR-0028, decision 3): the cloud publishes a location's
//! profile as events in its own log ([`Runtime::publish_profile`]), and every runtime takes the
//! latest publication of the roster's cloud that passes every check, between two intents, so no
//! intent ever sees half a profile.
//!
//! A runtime looks for publications when replication starts and after each frame, in the part of
//! the cloud's log it hasn't looked at yet, and when the shell asks with the roster before it
//! joins its network ([`Runtime::take_published`]), so a device restarted rings from what it
//! took even while it can't join. A publication is taken only from the roster's cloud,
//! only once it holds every chunk, and only if they join to the size and hash it names and decode
//! to a profile of the device's location. One taken replaces the profile before it, the
//! provisioned one first; a device restarted takes again, when replication starts, the latest
//! its store holds. An order begun under one version is priced under the one current when each
//! line is rung: every line records its catalog version.

use keel_domain::codec::ProfileVersion;
use keel_domain::profile::Profile;
use keel_domain::publication::{self, PublicationEvent, Published};
use keel_domain::roster::Roster;
use keel_domain::schema::DomainEvent;
use keel_events::envelope::{Actor, Component, Device, StreamRef};
use keel_events::keys::Signer;
use keel_types::{Clock, Entropy, Id};

use crate::error::RuntimeError;
use crate::runtime::{Meta, Runtime, append};

/// Whose publications a runtime takes: the cloud its roster names, and how far into its log the
/// runtime has looked.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Watch {
    cloud: Option<Id<Device>>,
    looked: u64,
}

/// How many of the cloud's events are read at once when looking for publications: chunks are up
/// to 192 KiB each.
const READ: u32 = 16;

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Publishes `profile` for the location's devices, as the location's cloud: its chunks and
    /// its publication, in one write, in a stream of its own; the runtime takes it at once, and
    /// replication carries it to every device. Answers with its version.
    ///
    /// # Errors
    /// [`RuntimeError::NotSyncing`] before replication starts; [`RuntimeError::NotCloud`] if this
    /// device isn't the roster's cloud; [`RuntimeError::RosterMismatch`] if the profile is another
    /// location's; [`RuntimeError::Publication`] if it is too large to publish;
    /// [`RuntimeError::Store`] if the store can't be written.
    pub fn publish_profile(&mut self, profile: &Profile) -> Result<ProfileVersion, RuntimeError> {
        let sync = self.sync.as_ref().ok_or(RuntimeError::NotSyncing)?;
        if sync.roster.cloud().is_none_or(|cloud| cloud.device != self.device) {
            return Err(RuntimeError::NotCloud);
        }
        // As every write: only once the cloud's own log is settled, or it could fork it. Found in
        // review: a cell restored from an older copy published before it heard the hub.
        if !sync.may_write(self.clock.now()) {
            return Err(RuntimeError::NotSettled);
        }
        if profile.data().location != self.profile.data().location {
            return Err(RuntimeError::RosterMismatch);
        }
        let events = publication::publish(profile)
            .map_err(|error| RuntimeError::Publication(error.to_string()))?;
        let now = self.clock.now();
        let meta = Meta {
            now,
            business_date: profile.business_day().business_date_of(now)?,
            actor: Actor::System(Component::new("cloud")?),
            correlation: self.ids.generate(now)?,
        };
        let stream = self.ids.generate(now)?;
        self.store.write(|w| {
            for event in &events {
                append(w, stream, event, &meta)?;
            }
            Ok::<_, RuntimeError>(())
        })?;
        self.take_publications()?;
        Ok(publication::version(profile))
    }

    /// Takes the latest publication of `roster`'s cloud that the store holds, before the runtime
    /// joins its network: a device restarted rings from what it took, even while it can't join.
    /// Answers whether it took one.
    ///
    /// # Errors
    /// [`RuntimeError::RosterMismatch`] if the roster is another location's;
    /// [`RuntimeError::Store`] if the store can't be read.
    pub fn take_published(&mut self, roster: &Roster) -> Result<bool, RuntimeError> {
        if roster.data().location != self.profile.data().location {
            return Err(RuntimeError::RosterMismatch);
        }
        self.watch_cloud(roster.cloud().map(|cloud| cloud.device));
        self.take_publications()
    }

    /// Takes publications from `cloud` from now on, looking at its log from the start if it is
    /// another cloud than before.
    pub(crate) fn watch_cloud(&mut self, cloud: Option<Id<Device>>) {
        if self.published.cloud != cloud {
            self.published = Watch { cloud, looked: 0 };
        }
    }

    /// Takes the latest publication of the cloud the runtime watches that the part of its log
    /// not yet looked at holds, and that passes every check; answers whether it took one.
    ///
    /// # Errors
    /// [`RuntimeError::Store`] if the store can't be read: nothing is taken, and the next look
    /// starts from where this one did.
    pub(crate) fn take_publications(&mut self) -> Result<bool, RuntimeError> {
        let Some(cloud) = self.published.cloud else { return Ok(false) };
        let mut looked = self.published.looked;
        let head = self.store.head(cloud)?.seq();
        // The publications not yet looked at, oldest first.
        let mut found: Vec<(StreamRef, Published)> = Vec::new();
        while looked < head {
            let events = self.store.log(cloud, looked, READ)?;
            let Some(last) = events.last().map(|event| event.body().origin_seq.get()) else {
                break;
            };
            for event in &events {
                let body = event.body();
                if body.stream.kind.as_str() != publication::STREAM
                    || !publication::is_published(&body.schema)
                {
                    continue;
                }
                // A publication that doesn't decode is never taken.
                if let Ok(PublicationEvent::Published(published)) =
                    PublicationEvent::decode(&body.schema, &body.payload)
                {
                    found.push((body.stream.clone(), published));
                }
            }
            looked = last;
        }
        // The latest that passes: in the cloud's own log, its order is its HLC's.
        let mut taken = None;
        for (stream, published) in found.iter().rev() {
            if let Some(profile) = self.joined(cloud, stream, published)? {
                taken = Some((profile, published.version()));
                break;
            }
        }
        self.published.looked = looked;
        let Some((profile, version)) = taken else { return Ok(false) };
        if version == self.profile_version {
            return Ok(false);
        }
        // A member the new team doesn't have is signed out.
        if self.member.is_some_and(|member| profile.member(member).is_none()) {
            self.member = None;
        }
        self.profile = profile;
        self.profile_version = version;
        Ok(true)
    }

    /// The profile `published` makes from the chunks `cloud` wrote in `stream`, if it passes
    /// every check and is this device's location's.
    fn joined(
        &self,
        cloud: Id<Device>,
        stream: &StreamRef,
        published: &Published,
    ) -> Result<Option<Profile>, RuntimeError> {
        let mut chunks = Vec::new();
        for event in self.store.stream(stream)? {
            let body = event.body();
            if body.origin_device != cloud {
                continue;
            }
            if let Ok(PublicationEvent::Chunk(chunk)) =
                PublicationEvent::decode(&body.schema, &body.payload)
            {
                chunks.push(chunk);
            }
        }
        Ok(publication::assemble(published, &chunks)
            .ok()
            .filter(|profile| profile.data().location == self.profile.data().location))
    }
}
