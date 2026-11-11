//! Staff in the runtime (ADR-0029, decision 5): signing in with a PIN, a manager's approval held
//! for the next intent that needs one, and the gate each intent passes.
//!
//! - **PINs**: every five wrong PINs on a device, whoever they were for, lock PIN entry for 30 s,
//!   then twice as long after each further five, up to 15 minutes, by the runtime's clock. Only 15
//!   minutes with no wrong PIN start the count again: a right one doesn't. Found in review: any
//!   right PIN started it again, so a cashier could guess at a manager's between their own.
//! - **Approvals**: an intent that needs one it doesn't have is refused,
//!   [`RuntimeError::ApprovalRequired`], and the runtime remembers what was asked: the action, its
//!   amount or percentage, and what it was on. [`Runtime::approve`] checks an approver's PIN, and
//!   that they may approve that request, and holds their approval for 60 s, for that request
//!   alone, of the member who asked. Found in review: an approval covered whatever gated intent
//!   came next.
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
/// How long with no wrong PIN starts the count of wrong PINs again.
pub const QUIET: Duration = Duration::from_secs(15 * 60);

/// A device's count of wrong PINs, and the lock they put on PIN entry.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PinGuard {
    wrong: u32,
    last_wrong: Option<Timestamp>,
    locked_until: Option<Timestamp>,
}

/// A request refused for want of an approval: what the member asked for, and on what.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Asked {
    actor: Id<TeamMember>,
    request: Request,
    subject: Option<Id<Aggregate>>,
}

/// An approval given for a request asked, waiting for the intent it is for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pending {
    approver: Id<TeamMember>,
    asked: Asked,
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

    /// Counts a wrong PIN at `now`, locking PIN entry after each [`PIN_TRIES`]. The count starts
    /// again after [`QUIET`] with none.
    fn wrong(&mut self, now: Timestamp) {
        let quiet = self
            .last_wrong
            .is_some_and(|last| now.as_millis().saturating_sub(last.as_millis()) > millis(QUIET));
        if quiet {
            self.wrong = 0;
        }
        self.last_wrong = Some(now);
        self.wrong = self.wrong.saturating_add(1);
        if self.wrong.is_multiple_of(PIN_TRIES) {
            let doublings = (self.wrong / PIN_TRIES).saturating_sub(1).min(16);
            let lock = FIRST_LOCK.saturating_mul(1 << doublings).min(LONGEST_LOCK);
            let until = Timestamp::from_millis(now.as_millis().saturating_add(millis(lock)));
            self.locked_until = Some(until.unwrap_or(now));
        }
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
        self.asked = None;
        Ok(())
    }

    /// Signs the team member out: intents are refused until another signs in.
    pub const fn sign_out(&mut self) {
        self.member = None;
        self.pending = None;
        self.asked = None;
    }

    /// `approver`'s approval, their PIN checked, of the request the member signed in was last
    /// refused for want of one, held for that request alone, within 60 s.
    ///
    /// # Errors
    /// [`RuntimeError::NotSignedIn`]; [`RuntimeError::NothingToApprove`] if no request waits for an
    /// approval; [`RuntimeError::NotApprover`] if the approver's role doesn't permit it, or it is
    /// their own; otherwise as [`Runtime::sign_in`].
    pub fn approve(&mut self, approver: Id<TeamMember>, pin: &str) -> Result<(), RuntimeError> {
        let actor = self.member.ok_or(RuntimeError::NotSignedIn)?;
        let asked = self
            .asked
            .filter(|asked| asked.actor == actor)
            .ok_or(RuntimeError::NothingToApprove)?;
        self.check_pin(approver, pin)?;
        if !self.profile.may_approve(approver, actor, &asked.request) {
            return Err(RuntimeError::NotApprover { approver, action: asked.request.action });
        }
        self.pending = Some(Pending { approver, asked, at: self.clock.now() });
        Ok(())
    }

    /// Who may approve the request waiting for an approval: the team members whose role permits
    /// it, but the one who asked.
    pub fn approvers(&self) -> Vec<Id<TeamMember>> {
        let Some(asked) = self.asked.filter(|asked| Some(asked.actor) == self.member) else {
            return Vec::new();
        };
        self.profile
            .data()
            .team
            .iter()
            .map(|member| member.id)
            .filter(|&member| self.profile.may_approve(member, asked.actor, &asked.request))
            .collect()
    }

    /// The approver, if an approval waits now for `asked`.
    fn waiting(&self, asked: &Asked) -> Option<Id<TeamMember>> {
        let pending = self.pending?;
        let age = self.clock.now().as_millis().saturating_sub(pending.at.as_millis());
        (pending.asked == *asked && (0..=millis(APPROVAL_LIFE)).contains(&age))
            .then_some(pending.approver)
    }

    /// Whether an approval waits now, for the member signed in.
    pub fn approval_waiting(&self) -> Option<Id<TeamMember>> {
        let pending = self.pending?;
        if Some(pending.asked.actor) != self.member {
            return None;
        }
        self.waiting(&pending.asked)
    }

    /// The gate an intent passes for `request` on `subject`: nothing, if the member signed in may
    /// do it; the approval to write with its events, if one waits for exactly that request.
    ///
    /// # Errors
    /// [`RuntimeError::NotSignedIn`]; [`RuntimeError::ApprovalRequired`] if it needs an approval
    /// and none waits for it, remembering what was asked; [`RuntimeError::NotApprover`] if the
    /// approver may no longer approve it.
    pub(crate) fn gate(
        &mut self,
        request: Request,
        subject: Option<Id<Aggregate>>,
    ) -> Result<Option<Granted>, RuntimeError> {
        let member = self.member.ok_or(RuntimeError::NotSignedIn)?;
        if self.profile.permits(member, &request) {
            return Ok(None);
        }
        let asked = Asked { actor: member, request, subject };
        let Some(approver) = self.waiting(&asked) else {
            self.asked = Some(asked);
            return Err(RuntimeError::ApprovalRequired(request.action));
        };
        // A profile taken since may have changed the approver's role.
        if !self.profile.may_approve(approver, member, &request) {
            self.pending = None;
            return Err(RuntimeError::NotApprover { approver, action: request.action });
        }
        Ok(Some(Granted::new(approver, request, subject)?))
    }

    /// An approval used by a write that succeeded: it is spent, with the request it answered.
    pub(crate) fn spend(&mut self, granted: Option<&Granted>) {
        if granted.is_some() {
            self.pending = None;
            self.asked = None;
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
