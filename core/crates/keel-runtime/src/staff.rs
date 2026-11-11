//! Staff in the runtime (ADR-0029, decision 5): signing in with a PIN, a manager's approval held
//! for the next intent that needs one, and the gate each intent passes.
//!
//! - **PINs**: five wrong in a row on a device lock PIN entry for 30 s, then twice as long after
//!   each further five, up to 15 minutes, by the runtime's clock. A right one starts the count
//!   again.
//! - **Approvals**: [`Runtime::approve`] checks an approver's PIN and holds their approval for 60 s,
//!   for the next intent that needs one, of the member signed in when it was given. An intent that
//!   needs one it doesn't have is refused, [`RuntimeError::ApprovalRequired`], so the shell can
//!   ask a manager and try again; one the approver's role doesn't permit, or that is their own, is
//!   refused, [`RuntimeError::NotApprover`].
//! - **The approval** is an `approval.granted` event, written in the intent's write, which the
//!   intent's events name in their envelope.

use core::time::Duration;

use keel_domain::staff::{ApprovalEvent, Granted, Request};
use keel_events::envelope::{Aggregate, Event, TeamMember};
use keel_events::keys::Signer;
use keel_store::Writing;
use keel_types::{Clock, Entropy, Id, Timestamp};

use crate::error::RuntimeError;
use crate::runtime::{Meta, Runtime, append};

/// How many wrong PINs in a row lock PIN entry.
pub const PIN_TRIES: u32 = 5;
/// How long the first lock lasts.
pub const FIRST_LOCK: Duration = Duration::from_secs(30);
/// The longest a lock lasts.
pub const LONGEST_LOCK: Duration = Duration::from_secs(15 * 60);
/// How long an approval waits for the intent it is for.
pub const APPROVAL_LIFE: Duration = Duration::from_secs(60);

/// A device's count of wrong PINs, and the lock they put on PIN entry.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PinGuard {
    wrong: u32,
    locked_until: Option<Timestamp>,
}

/// An approval given, waiting for the intent it is for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pending {
    approver: Id<TeamMember>,
    /// The member signed in when it was given: it is theirs alone.
    actor: Id<TeamMember>,
    at: Timestamp,
}

/// Milliseconds, as a clock counts them.
fn millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

impl PinGuard {
    /// Whether PIN entry is locked at `now`, and for how many seconds more.
    fn locked(&self, now: Timestamp) -> Option<u64> {
        let until = self.locked_until?;
        let left = until.as_millis().saturating_sub(now.as_millis());
        // A clock set back since the lock began leaves at most the longest lock.
        let left = left.min(millis(LONGEST_LOCK));
        (left > 0).then(|| u64::try_from(left.saturating_add(999) / 1000).unwrap_or(0))
    }

    /// Counts a wrong PIN at `now`, locking PIN entry after each [`PIN_TRIES`].
    fn wrong(&mut self, now: Timestamp) {
        self.wrong = self.wrong.saturating_add(1);
        if self.wrong.is_multiple_of(PIN_TRIES) {
            let doublings = (self.wrong / PIN_TRIES).saturating_sub(1).min(16);
            let lock = FIRST_LOCK.saturating_mul(1 << doublings).min(LONGEST_LOCK);
            let until = Timestamp::from_millis(now.as_millis().saturating_add(millis(lock)));
            self.locked_until = Some(until.unwrap_or(now));
        }
    }

    fn right(&mut self) {
        *self = PinGuard::default();
    }
}

impl<S: Signer, E: Entropy, C: Clock, I: Entropy> Runtime<S, E, C, I> {
    /// Checks `pin` is `member`'s, as PIN entry's lock allows.
    fn check_pin(&mut self, member: Id<TeamMember>, pin: &str) -> Result<(), RuntimeError> {
        if self.profile.member(member).is_none() {
            return Err(RuntimeError::UnknownMember(member));
        }
        let now = self.clock.now();
        if let Some(seconds) = self.pins.locked(now) {
            return Err(RuntimeError::LockedOut { seconds });
        }
        if self.profile.verifies_pin(member, pin) {
            self.pins.right();
            Ok(())
        } else {
            self.pins.wrong(now);
            Err(RuntimeError::WrongPin)
        }
    }

    /// Signs `member` in with their PIN: the events of every intent from now on record them. In
    /// a profile without roles, members have no PIN, and sign in with an empty one.
    ///
    /// # Errors
    /// [`RuntimeError::UnknownMember`] if the profile's team doesn't have them;
    /// [`RuntimeError::WrongPin`] if the PIN isn't theirs; [`RuntimeError::LockedOut`] while PIN
    /// entry is locked by wrong PINs.
    pub fn sign_in(&mut self, member: Id<TeamMember>, pin: &str) -> Result<(), RuntimeError> {
        self.check_pin(member, pin)?;
        self.member = Some(member);
        self.pending = None;
        Ok(())
    }

    /// Signs the team member out: intents are refused until another signs in.
    pub const fn sign_out(&mut self) {
        self.member = None;
        self.pending = None;
    }

    /// `approver`'s approval, their PIN checked, for the next intent that needs one of the member
    /// signed in, within 60 s.
    ///
    /// # Errors
    /// [`RuntimeError::NotSignedIn`] if no one is signed in to approve for; otherwise as
    /// [`Runtime::sign_in`].
    pub fn approve(&mut self, approver: Id<TeamMember>, pin: &str) -> Result<(), RuntimeError> {
        let actor = self.member.ok_or(RuntimeError::NotSignedIn)?;
        self.check_pin(approver, pin)?;
        self.pending = Some(Pending { approver, actor, at: self.clock.now() });
        Ok(())
    }

    /// Whether an approval waits now, for the member signed in.
    pub fn approval_waiting(&self) -> Option<Id<TeamMember>> {
        let pending = self.pending?;
        let age = self.clock.now().as_millis().saturating_sub(pending.at.as_millis());
        (Some(pending.actor) == self.member && (0..=millis(APPROVAL_LIFE)).contains(&age))
            .then_some(pending.approver)
    }

    /// The gate an intent passes for `request` on `subject`: nothing, if the member signed in may
    /// do it; the approval to write with its events, if one waits that permits it.
    ///
    /// # Errors
    /// [`RuntimeError::NotSignedIn`]; [`RuntimeError::ApprovalRequired`] if it needs an approval
    /// and none waits; [`RuntimeError::NotApprover`] if the one waiting doesn't permit it.
    pub(crate) fn gate(
        &self,
        request: Request,
        subject: Option<Id<Aggregate>>,
    ) -> Result<Option<Granted>, RuntimeError> {
        let member = self.member.ok_or(RuntimeError::NotSignedIn)?;
        if self.profile.permits(member, &request) {
            return Ok(None);
        }
        let approver =
            self.approval_waiting().ok_or(RuntimeError::ApprovalRequired(request.action))?;
        if !self.profile.may_approve(approver, member, &request) {
            return Err(RuntimeError::NotApprover { approver, action: request.action });
        }
        Ok(Some(Granted::new(approver, request, subject)?))
    }

    /// An approval used by a write that succeeded: it is spent.
    pub(crate) fn spend(&mut self, granted: Option<&Granted>) {
        if granted.is_some() {
            self.pending = None;
        }
    }
}

/// Writes `granted`, if there is one, in a stream of its own, and answers `meta` naming it, for
/// the events of the intent it approves.
pub(crate) fn approved<S: Signer, E: Entropy>(
    w: &mut Writing<'_, S, E>,
    granted: Option<&Granted>,
    stream: Option<Id<Aggregate>>,
    meta: &Meta,
) -> Result<Meta, RuntimeError> {
    let mut meta = meta.clone();
    if let (Some(granted), Some(stream)) = (granted, stream) {
        let approval: Id<Event> = append(w, stream, &ApprovalEvent::Granted(*granted), &meta)?;
        meta.approval = Some(approval);
    }
    Ok(meta)
}
