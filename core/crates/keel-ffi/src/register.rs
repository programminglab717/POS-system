//! The register object (ADR-0025, decision 2): one device runtime behind a lock, a method per
//! thing a cashier does, each answered with the whole view.

#![expect(
    clippy::needless_pass_by_value,
    reason = "UniFFI hands exported functions their arguments owned"
)]

use core::num::{NonZeroU8, NonZeroU32};
use core::time::Duration;
use std::net::ToSocketAddrs;
use std::sync::{Arc, Mutex, MutexGuard};

use keel_doc::{Columns, PrinterProfile};
use keel_domain::profile::{self, Profile};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_runtime::{Identity, Runtime, RuntimeError};
use keel_store::StoreKey;
use keel_types::{Id, Locale, Money, OsEntropy, SystemClock};

use crate::error::KeelError;
use crate::records::{
    Amount, CashPaid, Choice, Item, Job, Menu, Mode, Outcome, PrintJob, Printer, RegisterConfig,
    Resolution, Ticket, Waits,
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
}

/// `text`, an identifier.
pub(crate) fn id<T>(text: &str) -> Result<Id<T>, KeelError> {
    Id::parse(text).map_err(|_| KeelError::invalid(format!("{text:?} isn't an identifier")))
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
        .map_err(|error| KeelError::Device { reason: error.to_string() })?;
        Ok(Arc::new(Register { runtime: Mutex::new(runtime), locale }))
    }

    /// Signs `member` in: every event from now on records them.
    ///
    /// # Errors
    /// [`KeelError::UnknownMember`] if the team doesn't have them.
    pub fn sign_in(&self, member: String) -> Result<(), KeelError> {
        let member = id(&member)?;
        self.with(|runtime| runtime.sign_in(member))
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
        let mut runtime: MutexGuard<'_, DeviceRuntime> = self
            .runtime
            .lock()
            .map_err(|_| KeelError::Device { reason: "the register's lock was poisoned".into() })?;
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

/// The demo café's profile, encoded: for the shell before a location has its own, and for tests.
#[uniffi::export]
pub fn demo_profile() -> Vec<u8> {
    profile::demo().map(|demo| demo.encode()).unwrap_or_default()
}

/// Runs `job` against the printer at `host` and `port`, on the caller's thread and outside any
/// register's lock, and says what happened, for [`Register::print_outcome`].
#[uniffi::export]
pub fn print_tcp(host: String, port: u16, job: PrintJob, waits: Waits) -> Outcome {
    let waits = keel_print::Waits {
        status: Duration::from_millis(waits.status_ms),
        word: Duration::from_millis(waits.word_ms),
    };
    let address =
        (host.as_str(), port).to_socket_addrs().ok().and_then(|mut addresses| addresses.next());
    let process_id: Option<[u8; 4]> = job.process_id.as_slice().try_into().ok();
    match (address, process_id) {
        (Some(address), Some(id)) => {
            keel_print::tcp::print(address, &job.bytes, job.confirm.into(), id, waits).into()
        }
        _ => Outcome::NotSent { reason: crate::records::NotSent::Unreachable },
    }
}
