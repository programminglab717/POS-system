//! The register object (ADR-0025, decision 2): one device runtime behind a lock, a method per
//! thing a cashier does, each answered with the whole view.

#![expect(
    clippy::needless_pass_by_value,
    reason = "UniFFI hands exported functions their arguments owned"
)]

use core::num::{NonZeroU8, NonZeroU32};
use core::time::Duration;
use std::net::{TcpListener, ToSocketAddrs};
use std::sync::{Arc, Mutex, MutexGuard};

use keel_doc::{Columns, PrinterProfile};
use keel_domain::codec::{Note, ReasonCode};
use keel_domain::order::{DiscountValue, Reason};
use keel_domain::profile::{self, Profile};
use keel_domain::roster::Roster;
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_net::{Credentials, Net, NetError};
use keel_runtime::{Identity, Runtime, RuntimeError};
use keel_store::StoreKey;
use keel_types::{Clock, Entropy, Id, IdGenerator, Locale, Money, OsEntropy, SystemClock};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::error::KeelError;
use crate::records::{
    Amount, CashPaid, Choice, Item, Job, Member, Menu, Mode, NewDevice, Outcome, PrintJob, Printer,
    RegisterConfig, Resolution, SyncKit, SyncState, Ticket, Waits,
};

/// The runtime as a device runs it: the system's clock and the operating system's randomness,
/// the one place in the kernel that reads them.
type DeviceRuntime = Runtime<SoftwareSigner, OsEntropy, SystemClock, OsEntropy>;

/// A device's register: its store and its location's profile, the intents a cashier makes, and
/// the views the register shows.
///
/// Calls are serialized by a lock, so the register has one writer whatever the shell does; the
/// shell makes them from one background thread, never the UI's.
#[derive(uniffi::Object)]
pub struct Register {
    runtime: Mutex<DeviceRuntime>,
    locale: Locale,
    /// The device the register is.
    device: Id<Device>,
    /// The location's network, once the register joins it. Taken before `runtime` when both are.
    net: Mutex<Option<Net>>,
}

/// `text`, an identifier in its canonical form: lowercase, as every identifier the register gives
/// is, so that the shell compares what it holds with what it gets back.
pub(crate) fn id<T>(text: &str) -> Result<Id<T>, KeelError> {
    Id::parse(text).ok().filter(|id: &Id<T>| id.to_string() == text).ok_or_else(|| {
        KeelError::invalid(format!("{text:?} isn't an identifier in its canonical form"))
    })
}

/// A reason: the merchant's `code`, and an optional `note`.
fn reason(code: &str, note: Option<String>) -> Result<Reason, KeelError> {
    let code = ReasonCode::new(code)
        .map_err(|_| KeelError::invalid(format!("{code:?} isn't a reason code")))?;
    let note = note
        .map(|note| Note::new(&note).map_err(|_| KeelError::invalid("the note isn't valid")))
        .transpose()?;
    Ok(Reason { code, note })
}

/// The longest [`Register::sync_step`] waits for the network.
pub const MAX_SYNC_WAIT: Duration = Duration::from_millis(50);

/// The most events one [`Register::sync_step`] takes in.
const SYNC_TURNS: usize = 64;

/// The longest one [`Register::sync_step`] spends taking in what is waiting.
const SYNC_BUDGET: Duration = Duration::from_millis(10);

/// Turns the sync loop with `turn`: first waiting at most `wait`, then, while each turn took in
/// something, again without waiting, at most [`SYNC_TURNS`] turns in [`SYNC_BUDGET`]; how many it
/// turned. Found in review: one event a turn fell behind a hub's devices.
pub(crate) fn drain<E>(
    mut turn: impl FnMut(Duration) -> Result<bool, E>,
    wait: Duration,
) -> Result<usize, E> {
    let started = std::time::Instant::now();
    let mut took = turn(wait)?;
    let mut turns = 1_usize;
    while took && turns < SYNC_TURNS && started.elapsed() < SYNC_BUDGET {
        took = turn(Duration::ZERO)?;
        turns = turns.saturating_add(1);
    }
    Ok(turns)
}

/// A listener at `port` on every address: IPv6 and IPv4 where the system has both, IPv4 alone
/// where it has no IPv6.
fn listen(port: u16) -> Result<TcpListener, KeelError> {
    TcpListener::bind(("::", port))
        .or_else(|_| TcpListener::bind(("0.0.0.0", port)))
        .map_err(|error| KeelError::Device { reason: error.to_string() })
}

/// The device `roster` most prefers as the hub: the highest priority, then the lowest identifier,
/// as the election prefers them.
pub(crate) fn preferred_hub(roster: &Roster) -> Option<Id<Device>> {
    roster
        .data()
        .devices
        .iter()
        .filter_map(|device| device.hub.map(|priority| (priority, device.id)))
        .max_by(|(left, left_id), (right, right_id)| {
            left.cmp(right).then_with(|| right_id.to_bytes().cmp(&left_id.to_bytes()))
        })
        .map(|(_, id)| id)
}

/// `bytes`, 32 of them.
fn key_bytes(bytes: &[u8], what: &str) -> Result<[u8; 32], KeelError> {
    bytes.try_into().map_err(|_| KeelError::invalid(format!("{what} must be 32 bytes")))
}

/// The choices, a tree, from their flattened list: each names its parent's index, earlier in
/// the list.
pub(crate) fn choices(flat: &[Choice]) -> Result<Vec<profile::Choice>, KeelError> {
    let mut built: Vec<(Option<usize>, profile::Choice)> = Vec::with_capacity(flat.len());
    for (at, choice) in flat.iter().enumerate() {
        let quantity = NonZeroU8::new(choice.quantity)
            .ok_or_else(|| KeelError::invalid("a choice's quantity must be 1 or more"))?;
        let parent = match choice.parent {
            None => None,
            Some(parent) => {
                let parent = usize::try_from(parent).ok().filter(|&parent| parent < at);
                Some(parent.ok_or_else(|| {
                    KeelError::invalid("a choice's parent must be a choice before it")
                })?)
            }
        };
        let modifier = id(&choice.modifier)?;
        built.push((parent, profile::Choice { modifier, quantity, choices: Vec::new() }));
    }
    // From the last to the first, each choice moves under its parent, which comes before it.
    let mut roots = Vec::new();
    while let Some((parent, choice)) = built.pop() {
        match parent.and_then(|parent| built.get_mut(parent)) {
            Some((_, parent)) => parent.choices.insert(0, choice),
            None => roots.push(choice),
        }
    }
    roots.reverse();
    Ok(roots)
}

#[uniffi::export]
impl Register {
    /// Opens the device's register: its store at `config.path`, with its key, for the location
    /// `config.profile` describes.
    ///
    /// # Errors
    /// [`KeelError::InvalidArgument`] if a key isn't 32 bytes, the device isn't an identifier,
    /// the profile doesn't decode or the locale isn't one Keel knows; [`KeelError::Device`] if
    /// the store can't be opened with the key.
    #[uniffi::constructor]
    pub fn open(config: RegisterConfig) -> Result<Arc<Register>, KeelError> {
        let mut store_key = key_bytes(&config.store_key, "the store's key")?;
        let signing_key = key_bytes(&config.signing_key, "the signing key")?;
        let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &signing_key)
            .map_err(KeelError::invalid)?;
        let profile = Profile::decode(&config.profile).map_err(KeelError::invalid)?;
        let locale = Locale::from_tag(&config.locale).map_err(KeelError::invalid)?;
        let device: Id<Device> = id(&config.device)?;
        let runtime = Runtime::open(
            &config.path,
            StoreKey::new(&mut store_key),
            Identity { device, signer },
            profile,
            SystemClock,
            OsEntropy,
            OsEntropy,
            locale,
        )
        .map_err(|error| match error {
            RuntimeError::Store(store) => KeelError::from(store),
            other => KeelError::Device { reason: other.to_string() },
        })?;
        Ok(Arc::new(Register {
            runtime: Mutex::new(runtime),
            locale,
            device,
            net: Mutex::new(None),
        }))
    }

    /// Signs `member` in with their PIN: every event from now on records them. In a profile
    /// without roles, members have no PIN, and sign in with an empty one (ADR-0029).
    ///
    /// # Errors
    /// [`KeelError::UnknownMember`] if the team doesn't have them; [`KeelError::WrongPin`] if the
    /// PIN isn't theirs; [`KeelError::LockedOut`] while PIN entry is locked by wrong PINs.
    pub fn sign_in(&self, member: String, pin: String) -> Result<(), KeelError> {
        let member = id(&member)?;
        self.with(|runtime| runtime.sign_in(member, &pin))
    }

    /// `approver`'s approval, their PIN checked, for the next intent of the member signed in
    /// that needs one, within a minute: ask for it when an intent answers
    /// [`KeelError::ApprovalRequired`], then try the intent again.
    ///
    /// # Errors
    /// [`KeelError::NotSignedIn`]; otherwise as [`Register::sign_in`].
    pub fn approve(&self, approver: String, pin: String) -> Result<(), KeelError> {
        let approver = id(&approver)?;
        self.with(|runtime| runtime.approve(approver, &pin))
    }

    /// Signs the team member out.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the register's lock was poisoned.
    pub fn sign_out(&self) -> Result<(), KeelError> {
        self.with(|runtime| {
            runtime.sign_out();
            Ok(())
        })
    }

    /// Starts an order at the register.
    ///
    /// # Errors
    /// [`KeelError::NotSignedIn`], and [`KeelError::Device`] if it can't be written.
    pub fn start_order(&self, mode: Mode) -> Result<Ticket, KeelError> {
        self.with(|runtime| runtime.start_order(mode.into())).map(|ticket| (&ticket).into())
    }

    /// Rings `variant`, with `choices`, `quantity` times on `order`.
    ///
    /// # Errors
    /// [`KeelError::Catalog`] if the catalog refuses the choices, [`KeelError::Refused`] if the
    /// order refuses the line, and the errors every intent can meet.
    pub fn add_item(
        &self,
        order: String,
        variant: String,
        choices: Vec<Choice>,
        quantity: u32,
    ) -> Result<Ticket, KeelError> {
        let (order, variant) = (id(&order)?, id(&variant)?);
        let choices = self::choices(&choices)?;
        let quantity = positive(quantity)?;
        self.with(|runtime| runtime.add_item(order, variant, &choices, quantity))
            .map(|ticket| (&ticket).into())
    }

    /// Changes how many of `line` `order` has.
    ///
    /// # Errors
    /// As [`Register::add_item`].
    pub fn change_quantity(
        &self,
        order: String,
        line: String,
        quantity: u32,
    ) -> Result<Ticket, KeelError> {
        let (order, line, quantity) = (id(&order)?, id(&line)?, positive(quantity)?);
        self.with(|runtime| runtime.change_quantity(order, line, quantity))
            .map(|ticket| (&ticket).into())
    }

    /// Takes `line` off `order`.
    ///
    /// # Errors
    /// As [`Register::add_item`].
    pub fn remove_line(&self, order: String, line: String) -> Result<Ticket, KeelError> {
        let (order, line) = (id(&order)?, id(&line)?);
        self.with(|runtime| runtime.remove_line(order, line)).map(|ticket| (&ticket).into())
    }

    /// Takes `basis_points` (from 1, 0.01%, to 10,000, 100%) off `line` of `order`, or off the
    /// whole order when `line` is `None`, for `reason`, the merchant's code, with an optional
    /// `note` (ADR-0030). A manager approves one beyond the member's role.
    ///
    /// # Errors
    /// [`KeelError::InvalidArgument`] if an identifier, the reason or the note isn't valid;
    /// [`KeelError::ApprovalRequired`] if it needs an approval; and as [`Register::add_item`].
    pub fn apply_percent_discount(
        &self,
        order: String,
        line: Option<String>,
        basis_points: u32,
        reason: String,
        note: Option<String>,
    ) -> Result<Ticket, KeelError> {
        let (order, line) = (id(&order)?, line.as_deref().map(id).transpose()?);
        let (value, reason) = (DiscountValue::Percent(basis_points), self::reason(&reason, note)?);
        self.with(|runtime| runtime.apply_discount(order, line, value, reason))
            .map(|ticket| (&ticket).into())
    }

    /// Takes `amount`, in the location's currency's minor units, off `line` of `order`, for
    /// `reason`, with an optional `note` (ADR-0030). A whole order takes only a percentage.
    ///
    /// # Errors
    /// As [`Register::apply_percent_discount`].
    pub fn apply_amount_discount(
        &self,
        order: String,
        line: String,
        amount: i64,
        reason: String,
        note: Option<String>,
    ) -> Result<Ticket, KeelError> {
        let (order, line) = (id(&order)?, id(&line)?);
        let reason = self::reason(&reason, note)?;
        self.with(|runtime| {
            let amount = Money::from_minor(amount, runtime.profile().data().currency);
            runtime.apply_discount(order, Some(line), DiscountValue::Amount(amount), reason)
        })
        .map(|ticket| (&ticket).into())
    }

    /// Prices `line` of `order` again at `unit_price`, in the location's currency's minor units,
    /// for `reason`, with an optional `note` (ADR-0030). A manager approves one beyond the
    /// member's role.
    ///
    /// # Errors
    /// As [`Register::apply_percent_discount`].
    pub fn reprice_line(
        &self,
        order: String,
        line: String,
        unit_price: i64,
        reason: String,
        note: Option<String>,
    ) -> Result<Ticket, KeelError> {
        let (order, line) = (id(&order)?, id(&line)?);
        let reason = self::reason(&reason, note)?;
        self.with(|runtime| {
            let price = Money::from_minor(unit_price, runtime.profile().data().currency);
            runtime.reprice_line(order, line, price, reason)
        })
        .map(|ticket| (&ticket).into())
    }

    /// Sets `line`'s notes, or clears them when `notes` is `None`, while it waits to be fired.
    ///
    /// # Errors
    /// [`KeelError::InvalidArgument`] if an identifier or the notes aren't valid; and as
    /// [`Register::add_item`].
    pub fn set_line_notes(
        &self,
        order: String,
        line: String,
        notes: Option<String>,
    ) -> Result<Ticket, KeelError> {
        let (order, line) = (id(&order)?, id(&line)?);
        let notes = notes
            .map(|notes| {
                Note::new(&notes).map_err(|_| KeelError::invalid("the notes aren't valid"))
            })
            .transpose()?;
        self.with(|runtime| runtime.set_line_notes(order, line, notes))
            .map(|ticket| (&ticket).into())
    }

    /// Takes `discount` off `order`.
    ///
    /// # Errors
    /// As [`Register::add_item`].
    pub fn remove_discount(&self, order: String, discount: String) -> Result<Ticket, KeelError> {
        let (order, discount) = (id(&order)?, id(&discount)?);
        self.with(|runtime| runtime.remove_discount(order, discount)).map(|ticket| (&ticket).into())
    }

    /// Drops `order`, its lines taken off first.
    ///
    /// # Errors
    /// As [`Register::add_item`].
    pub fn abandon(&self, order: String) -> Result<Ticket, KeelError> {
        let order = id(&order)?;
        self.with(|runtime| runtime.abandon(order)).map(|ticket| (&ticket).into())
    }

    /// Takes `tendered`, in the location's currency's minor units, in cash for `order`, and
    /// closes it; the drawer's kick and the receipt are queued to print.
    ///
    /// # Errors
    /// [`KeelError::TenderShort`] if it is short of what is due in cash,
    /// [`KeelError::NegativeTender`], and the errors every intent can meet.
    pub fn pay_cash(&self, order: String, tendered: i64) -> Result<CashPaid, KeelError> {
        let order = id(&order)?;
        self.with(|runtime| {
            let currency = runtime.profile().data().currency;
            runtime.pay_cash(order, Money::from_minor(tendered, currency))
        })
        .map(|paid| CashPaid { change: (&paid.change).into(), ticket: (&paid.ticket).into() })
    }

    /// Takes `tendered` in cash toward `check` of `order`: `part` of what it owes, in the
    /// location's currency's minor units, or all of it when `part` is `None` (ADR-0030). The
    /// check closes once it is paid, and the order with its last check.
    ///
    /// # Errors
    /// As [`Register::pay_cash`].
    pub fn pay_check_cash(
        &self,
        order: String,
        check: String,
        part: Option<i64>,
        tendered: i64,
    ) -> Result<CashPaid, KeelError> {
        let (order, check) = (id(&order)?, id(&check)?);
        self.with(|runtime| {
            let currency = runtime.profile().data().currency;
            let part = part.map(|part| Money::from_minor(part, currency));
            runtime.pay_check_cash(order, check, part, Money::from_minor(tendered, currency))
        })
        .map(|paid| CashPaid { change: (&paid.change).into(), ticket: (&paid.ticket).into() })
    }

    /// Recalls `order`, parked on another device, to this one: asks the hub for it (ADR-0030).
    /// The ticket says the device owns it once the hub's answer has come back.
    ///
    /// # Errors
    /// As [`Register::add_item`].
    pub fn recall(&self, order: String) -> Result<Ticket, KeelError> {
        let order = id(&order)?;
        self.with(|runtime| runtime.recall(order)).map(|ticket| (&ticket).into())
    }

    /// Takes `order` from the device that owns it without the hub, on a manager's word, for
    /// `reason`, with an optional `note` (ADR-0021): for when the hub can't be reached.
    ///
    /// # Errors
    /// [`KeelError::ApprovalRequired`] if it needs an approval; and as [`Register::add_item`].
    pub fn take_over(
        &self,
        order: String,
        reason: String,
        note: Option<String>,
    ) -> Result<Ticket, KeelError> {
        let (order, reason) = (id(&order)?, self::reason(&reason, note)?);
        self.with(|runtime| runtime.take_over(order, reason)).map(|ticket| (&ticket).into())
    }

    /// The device this register is: the owner a ticket names when this register owns it.
    pub fn device_id(&self) -> String {
        self.device.to_string()
    }

    /// Splits every live line of `order` evenly among `ways` checks, from 2 to 20 (ADR-0030).
    ///
    /// # Errors
    /// [`KeelError::Refused`] if it can't be split that way; and as [`Register::add_item`].
    pub fn split_evenly(&self, order: String, ways: u8) -> Result<Ticket, KeelError> {
        let order = id(&order)?;
        self.with(|runtime| runtime.split_evenly(order, ways)).map(|ticket| (&ticket).into())
    }

    /// Moves `line` of `order`, whole, to the open check `to`, or to a new one when `to` is
    /// `None` (ADR-0030).
    ///
    /// # Errors
    /// As [`Register::add_item`].
    pub fn move_line(
        &self,
        order: String,
        line: String,
        to: Option<String>,
    ) -> Result<Ticket, KeelError> {
        let (order, line, to) = (id(&order)?, id(&line)?, to.as_deref().map(id).transpose()?);
        self.with(|runtime| runtime.move_line(order, line, to)).map(|ticket| (&ticket).into())
    }

    /// The ticket of `order`.
    ///
    /// # Errors
    /// [`KeelError::UnknownOrder`] if the store holds no such order.
    pub fn ticket(&self, order: String) -> Result<Ticket, KeelError> {
        let order = id(&order)?;
        self.with(|runtime| runtime.ticket(order)).map(|ticket| (&ticket).into())
    }

    /// The tickets of the orders still open, oldest first.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the store can't be read.
    pub fn open_orders(&self) -> Result<Vec<Ticket>, KeelError> {
        self.with(|runtime| runtime.open_orders())
            .map(|tickets| tickets.iter().map(Ticket::from).collect())
    }

    /// Joins the location's network with `kit` (ADR-0026): replicates the store with the
    /// roster's devices, over `keel-net`. The device the roster most prefers as the hub (the
    /// highest priority, then the lowest identifier) listens at the roster's port; every other
    /// device connects to the roster's address. Call [`Register::sync_step`] often after.
    ///
    /// # Errors
    /// [`KeelError::InvalidArgument`] if the roster doesn't decode, isn't this device's location's
    /// or doesn't have it, the register syncs already, or the credentials aren't usable;
    /// [`KeelError::Device`] if the store can't be read or the network can't start.
    pub fn start_sync(&self, kit: SyncKit) -> Result<(), KeelError> {
        let roster = Roster::decode(&kit.roster).map_err(KeelError::invalid)?;
        let mut net = self.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if net.is_some() {
            return Err(KeelError::invalid("the register is joined to its network already"));
        }
        let credentials = Credentials {
            ca: CertificateDer::from(kit.ca),
            chain: vec![CertificateDer::from(kit.certificate)],
            key: PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(kit.tls_key)),
        };
        // The latest publication the store holds, before anything that can fail to start: a
        // register that can't join yet rings from what it took. Found in review.
        self.with(|runtime| runtime.take_published(&roster))?;
        let listens = preferred_hub(&roster) == Some(self.device);
        // The network first: if it can't start, as when the port is taken, the runtime isn't left
        // replicating with nothing to replicate over, and a later call can try again.
        let started = if listens {
            Net::hub(listen(roster.data().hub.port.get())?, roster.clone(), credentials)
        } else {
            Net::device(roster.clone(), credentials)
        }
        .map_err(|error| match error {
            NetError::Io(io) => KeelError::Device { reason: io.to_string() },
            other => KeelError::invalid(other),
        })?;
        // Only the device that listens at the roster's address may take the hub's role: the
        // others can't be reached there.
        let frames = self.with(|runtime| runtime.start_sync_as(roster, listens))?;
        for frame in frames {
            started.send(frame.to, frame.frame);
        }
        *net = Some(started);
        Ok(())
    }

    /// One turn of the loop that joins the register to its network: takes in what arrived,
    /// waiting at most `wait_ms` for it, ticks replication when it asks, and sends what it
    /// answers. The register's other calls wait while it does: keep `wait_ms` short, or 0.
    ///
    /// # Errors
    /// [`KeelError::InvalidArgument`] if the register hasn't joined its network;
    /// [`KeelError::Device`] if the store can't be read or written. Nothing a frame held was
    /// stored, and its peer sends it again: carry on.
    pub fn sync_step(&self, wait_ms: u32) -> Result<(), KeelError> {
        let net = self.net.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(net) = net.as_ref() else {
            return Err(KeelError::invalid("the register hasn't joined its network"));
        };
        // The wait is capped, as the register's other calls wait for it. Found in review: an
        // uncapped wait held the register's lock for as long as it was asked to.
        let wait = Duration::from_millis(u64::from(wait_ms)).min(MAX_SYNC_WAIT);
        self.with(|runtime| drain(|wait| keel_hub::step(runtime, net, wait), wait).map(|_| ()))
    }

    /// Who may approve what the member signed in was last refused for want of an approval: the
    /// team members whose role permits it, but the one who asked. Empty when nothing waits.
    pub fn approvers(&self) -> Vec<String> {
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .approvers()
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// The version of the profile the register rings from, 64 hexadecimal digits: the one
    /// provisioned until it takes one the cloud publishes. When it changes, the menu and the team
    /// are read again.
    pub fn profile_version(&self) -> String {
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .profile_version()
            .to_hex()
    }

    /// How replication stands, once the register has joined its network.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the store can't be read.
    pub fn sync_status(&self) -> Result<Option<SyncState>, KeelError> {
        let listening = self
            .net
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .is_some_and(|net| net.local_addr().is_some());
        let (status, version) =
            self.with(|runtime| Ok((runtime.sync_status()?, runtime.profile_version())))?;
        let profile_version = version.to_hex();
        Ok(status.map(|status| SyncState {
            listening,
            settled: status.settled,
            hub_reachable: status.hub_reachable,
            is_hub: status.is_hub,
            serving: status.serving,
            backed_up: status.backed_up,
            head: status.head,
            profile_version,
        }))
    }

    /// The tickets of the orders begun on today's business date, whatever became of them, oldest
    /// first.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the store can't be read.
    pub fn orders_today(&self) -> Result<Vec<Ticket>, KeelError> {
        self.with(|runtime| runtime.orders_today())
            .map(|tickets| tickets.iter().map(Ticket::from).collect())
    }

    /// What the register offers as cash tendered for `order`: what it owes in cash, then round
    /// amounts above it, at most five.
    ///
    /// # Errors
    /// [`KeelError::InvalidArgument`] if `order` isn't an identifier, [`KeelError::UnknownOrder`]
    /// if the store holds no such order.
    pub fn cash_offers(&self, order: String) -> Result<Vec<Amount>, KeelError> {
        let order = id(&order)?;
        self.with(|runtime| runtime.cash_offers(order))
            .map(|offers| offers.iter().map(Amount::from).collect())
    }

    /// `minor` of the location's currency, as the register shows it: for an amount a cashier
    /// types.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the register's lock was poisoned.
    pub fn amount(&self, minor: i64) -> Result<Amount, KeelError> {
        self.with(|runtime| Ok(runtime.amount(minor))).map(|amount| (&amount).into())
    }

    /// The team members who may sign in, in the profile's order.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the register's lock was poisoned.
    pub fn team(&self) -> Result<Vec<Member>, KeelError> {
        self.with(|runtime| {
            let profile = runtime.profile();
            Ok(runtime
                .team()
                .iter()
                .map(|member| Member {
                    id: member.id.to_string(),
                    name: member.name.to_string(),
                    role: member
                        .role
                        .and_then(|role| profile.role(role))
                        .map(|role| role.name.to_string()),
                    has_pin: member.pin.is_some(),
                })
                .collect())
        })
    }

    /// The menu.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the register's lock was poisoned.
    pub fn menu(&self) -> Result<Menu, KeelError> {
        self.with(|runtime| Ok(runtime.menu())).map(|menu| (&menu).into())
    }

    /// `variant`, with its modifier groups.
    ///
    /// # Errors
    /// [`KeelError::Catalog`] if the catalog has no such variant.
    pub fn item(&self, variant: String) -> Result<Item, KeelError> {
        let variant = id(&variant)?;
        self.with(|runtime| runtime.item(variant)).map(|item| (&item).into())
    }

    /// Sets the printer jobs go to.
    ///
    /// # Errors
    /// [`KeelError::InvalidArgument`] for columns outside 24 to 64.
    pub fn set_printer(&self, printer: Printer) -> Result<(), KeelError> {
        let columns = Columns::new(printer.columns)
            .ok_or_else(|| KeelError::invalid("a printer has 24 to 64 columns"))?;
        let profile =
            PrinterProfile { columns, cutter: printer.cutter, confirm: printer.confirm.into() };
        self.with(|runtime| {
            runtime.set_printer(profile);
            Ok(())
        })
    }

    /// The next job for the driver, if one is due and none is with it.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the store can't be read or written.
    pub fn next_print_job(&self) -> Result<Option<PrintJob>, KeelError> {
        let job = self.with(DeviceRuntime::next_print_job)?;
        Ok(job.map(|job| PrintJob {
            job: job.job.to_string(),
            kind: job.kind.into(),
            bytes: job.bytes,
            confirm: job.confirm.into(),
            process_id: job.process_id.to_vec(),
        }))
    }

    /// Records what the driver says happened to `job`.
    ///
    /// # Errors
    /// [`KeelError::UnknownJob`] unless it is the job with the driver.
    pub fn print_outcome(&self, job: String, outcome: Outcome) -> Result<(), KeelError> {
        let job = id(&job)?;
        self.with(|runtime| runtime.print_outcome(job, outcome.into()))
    }

    /// What staff decide about the unconfirmed `job`; the new job, when there is one.
    ///
    /// # Errors
    /// [`KeelError::NotUnconfirmed`] if it isn't unconfirmed.
    pub fn resolve(
        &self,
        job: String,
        resolution: Resolution,
    ) -> Result<Option<String>, KeelError> {
        let job = id(&job)?;
        self.with(|runtime| runtime.resolve(job, resolution.into()))
            .map(|again| again.map(|job| job.to_string()))
    }

    /// The receipt the closed `order` printed, printed again marked as a reprint.
    ///
    /// # Errors
    /// [`KeelError::NoReceipt`] if it printed none.
    pub fn reprint(&self, order: String) -> Result<String, KeelError> {
        let order = id(&order)?;
        self.with(|runtime| runtime.reprint(order)).map(|job| job.to_string())
    }

    /// The jobs queued, with the driver, and unconfirmed.
    ///
    /// # Errors
    /// [`KeelError::Device`] if the store can't be read.
    pub fn print_jobs(&self) -> Result<Vec<Job>, KeelError> {
        self.with(|runtime| runtime.print_jobs()).map(|jobs| jobs.iter().map(Job::from).collect())
    }
}

impl Register {
    /// Runs `call` on the runtime, under the lock, and maps its error.
    fn with<T>(
        &self,
        call: impl FnOnce(&mut DeviceRuntime) -> Result<T, RuntimeError>,
    ) -> Result<T, KeelError> {
        // A call that panicked left the lock poisoned. What it was writing was rolled back, as every
        // write of the store is a transaction, so the register carries on.
        let mut runtime: MutexGuard<'_, DeviceRuntime> =
            self.runtime.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        call(&mut runtime).map_err(|error| match error {
            // What is due, with its text.
            RuntimeError::TenderShort { due } => KeelError::TenderShort {
                due: Amount::from(&keel_runtime::Amount::of(due, self.locale)),
            },
            other => other.into(),
        })
    }
}

/// `n`, which must be 1 or more.
fn positive(n: u32) -> Result<NonZeroU32, KeelError> {
    NonZeroU32::new(n).ok_or_else(|| KeelError::invalid("a quantity must be 1 or more"))
}

/// A new device's identity, its identifier and keys drawn from the operating system's
/// randomness, each on its own.
///
/// # Errors
/// [`KeelError::Device`] if the operating system's randomness fails.
#[uniffi::export]
pub fn new_device() -> Result<NewDevice, KeelError> {
    let failed = |error: &dyn core::fmt::Display| KeelError::Device { reason: error.to_string() };
    let device: Id<Device> =
        IdGenerator::new(OsEntropy).generate(SystemClock.now()).map_err(|error| failed(&error))?;
    let mut entropy = OsEntropy;
    let mut key = || -> Result<Vec<u8>, KeelError> {
        let mut bytes = Vec::with_capacity(32);
        for _ in 0..4 {
            bytes.extend(entropy.next_u64().map_err(|error| failed(&error))?.to_be_bytes());
        }
        Ok(bytes)
    };
    Ok(NewDevice { device: device.to_string(), signing_key: key()?, store_key: key()? })
}

/// The demo café's profile, encoded: for the shell before a location has its own, and for tests.
#[uniffi::export]
pub fn demo_profile() -> Vec<u8> {
    profile::demo().map(|demo| demo.encode()).unwrap_or_default()
}

/// Runs `job` against the printer at `host` and `port`, on the caller's thread and outside any
/// register's lock, and says what happened, for [`Register::print_outcome`]. Each address the
/// host resolves to is tried in turn while none is reached. Resolving a name can take as long as
/// the system's resolver takes: an address is quicker.
///
/// # Errors
/// [`KeelError::InvalidArgument`] if the job's process ID isn't four bytes from `0x20` to
/// `0x7E`, as no job the register gives has.
#[uniffi::export]
pub fn print_tcp(
    host: String,
    port: u16,
    job: PrintJob,
    waits: Waits,
) -> Result<Outcome, KeelError> {
    let waits = keel_print::Waits {
        status: Duration::from_millis(waits.status_ms),
        word: Duration::from_millis(waits.word_ms),
    };
    let id: [u8; 4] = job
        .process_id
        .as_slice()
        .try_into()
        .ok()
        .filter(|id: &[u8; 4]| id.iter().all(|byte| (0x20..=0x7E).contains(byte)))
        .ok_or_else(|| KeelError::invalid("a process ID is four printable bytes"))?;
    let unreachable = keel_print::Outcome::NotSent(keel_print::NotSent::Unreachable);
    let mut outcome = unreachable;
    for address in (host.as_str(), port).to_socket_addrs().into_iter().flatten() {
        outcome = keel_print::tcp::print(address, &job.bytes, job.confirm.into(), id, waits);
        if outcome != unreachable {
            break;
        }
    }
    Ok(outcome.into())
}
