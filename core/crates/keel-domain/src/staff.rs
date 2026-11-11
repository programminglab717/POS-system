//! Staff, roles and approvals (ADR-0029): who may do what, up to what limit, and the approvals
//! that let a member do what their role alone doesn't.
//!
//! - **A PIN** ([`PinHash`]) is 4 to 8 digits, kept as its Argon2id hash: version 0x13, 19 MiB of
//!   memory, 2 passes, 1 lane, 32 bytes, with a 16-byte salt drawn for it.
//! - **A role** ([`Role`]) lists the actions its members may take, each with its limits
//!   ([`Permission`]): the most amount, in minor units of the location's currency, and the most
//!   percentage, in basis points, each absent when unlimited.
//! - **A request** ([`Request`]) is an action with the amount or percentage it is for. A role
//!   permits it if it lists the action and the request is within its limits.
//! - **An approval** is an event, `approval.granted` v1, in a stream of its own of kind
//!   [`STREAM`], written with the events of the action it approves, which name it in their
//!   envelope.
//!
//! | Schema | Keys |
//! |---|---|
//! | `approval.granted` | 1 approver, 2 action, 3 amount (absent if none), 4 percent (absent if none), 5 subject (absent if none) |
//!
//! A role, in a profile: `{1: identifier, 2: name, 3: permissions}`; a permission
//! `{1: action, 2: most amount, 3: most percent}`; a PIN `{1: salt, 2: hash}`.

use core::fmt;

use argon2::{Algorithm, Argon2, Params, Version};
use keel_events::cbor::Value;
use keel_events::envelope::{Aggregate, SchemaRef, TeamMember};
use keel_types::{Id, Money};

use crate::codec::{Field, Fields, Name, PayloadError, Record, code_enum};
use crate::refs::Role as RoleRef;
use crate::schema::{DecodeError, DomainEvent, SchemaId};

#[cfg(test)]
mod tests;

/// The kind of stream an approval is.
pub const STREAM: &str = "approval";

/// The most basis points a percentage may be: 100%.
pub const MAX_PERCENT: u32 = 10_000;

/// The fewest digits a PIN has.
pub const MIN_PIN: usize = 4;
/// The most.
pub const MAX_PIN: usize = 8;

/// Argon2id's memory, in KiB: 19 MiB.
const MEMORY: u32 = 19 * 1024;
/// Its passes.
const PASSES: u32 = 2;
/// Its lanes.
const LANES: u32 = 1;

const GRANTED: SchemaId = SchemaId { name: "approval.granted", version: 1 };

code_enum! {
    /// What a member may do, or be approved to do (ADR-0029, decision 3).
    pub enum Action {
        /// Abandon an order with lines on it.
        OrderVoid = 0,
        /// Take a line off an order.
        LineRemove = 1,
        /// Discount a line or an order, up to an amount and a percentage.
        DiscountApply = 2,
        /// Ring an item at another price than the catalog's, up to an amount.
        PriceOverride = 3,
        /// Refund, up to an amount.
        RefundIssue = 4,
        /// Open the cash drawer with no sale.
        DrawerNoSale = 5,
        /// Close a drawer over or short, by up to an amount.
        DrawerVariance = 6,
        /// Pay out of the drawer, up to an amount.
        DrawerPaidOut = 7,
        /// Print a receipt again.
        ReceiptReprint = 8,
        /// Mark an item available or not (86).
        ItemAvailability = 9,
    }
}

impl Action {
    /// The action's name, as the back office and the shells show it: `order.void`.
    pub const fn name(self) -> &'static str {
        match self {
            Action::OrderVoid => "order.void",
            Action::LineRemove => "line.remove",
            Action::DiscountApply => "discount.apply",
            Action::PriceOverride => "price.override",
            Action::RefundIssue => "refund.issue",
            Action::DrawerNoSale => "drawer.no_sale",
            Action::DrawerVariance => "drawer.variance",
            Action::DrawerPaidOut => "drawer.paid_out",
            Action::ReceiptReprint => "receipt.reprint",
            Action::ItemAvailability => "item.availability",
        }
    }

    /// The action named `name`.
    pub fn named(name: &str) -> Option<Action> {
        Action::ALL.iter().copied().find(|action| action.name() == name)
    }

    /// Whether the action is limited by an amount.
    pub const fn has_amount(self) -> bool {
        matches!(
            self,
            Action::DiscountApply
                | Action::PriceOverride
                | Action::RefundIssue
                | Action::DrawerVariance
                | Action::DrawerPaidOut
        )
    }

    /// Whether the action is limited by a percentage.
    pub const fn has_percent(self) -> bool {
        matches!(self, Action::DiscountApply)
    }
}

/// An action a role permits, with its limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Permission {
    /// The action.
    pub action: Action,
    /// The most amount, in minor units of the location's currency; `None` for no limit.
    pub most_amount: Option<u64>,
    /// The most percentage, in basis points; `None` for no limit.
    pub most_percent: Option<u32>,
}

/// A role: the actions its members may take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Role {
    /// Its identifier.
    pub id: Id<RoleRef>,
    /// Its name, as the back office shows it.
    pub name: Name,
    /// What its members may do: each action at most once.
    pub permissions: Vec<Permission>,
}

/// An action asked for, with the amount or percentage it is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    /// The action.
    pub action: Action,
    /// The amount it is for, if it is for one.
    pub amount: Option<Money>,
    /// The percentage it is for, in basis points, if it is for one.
    pub percent: Option<u32>,
}

impl Request {
    /// A request for `action` alone.
    pub const fn of(action: Action) -> Request {
        Request { action, amount: None, percent: None }
    }
}

impl Role {
    /// Whether the role permits `request`: it lists the action, and the request is within its
    /// limits. An amount counts by its size, whatever its sign.
    pub fn permits(&self, request: &Request) -> bool {
        let Some(permission) =
            self.permissions.iter().find(|permission| permission.action == request.action)
        else {
            return false;
        };
        let amount = match (request.amount, permission.most_amount) {
            (Some(amount), Some(most)) => amount.minor().unsigned_abs() <= most,
            _ => true,
        };
        let percent = match (request.percent, permission.most_percent) {
            (Some(percent), Some(most)) => percent <= most,
            _ => true,
        };
        amount && percent
    }

    /// Checks the role: each action once, and limits only where the action has them, within
    /// their range.
    ///
    /// # Errors
    /// The rule the role breaks.
    pub fn check(&self) -> Result<(), &'static str> {
        let mut seen = [false; Action::ALL.len()];
        for permission in &self.permissions {
            let at = usize::try_from(permission.action.code()).map_err(|_| "an unknown action")?;
            let slot = seen.get_mut(at).ok_or("an unknown action")?;
            if *slot {
                return Err("a role that lists an action twice");
            }
            *slot = true;
            if permission.most_amount.is_some() && !permission.action.has_amount() {
                return Err("an amount limit on an action that has none");
            }
            if permission.most_amount.is_some_and(|most| i64::try_from(most).is_err()) {
                return Err("an amount limit beyond any amount");
            }
            if permission.most_percent.is_some() && !permission.action.has_percent() {
                return Err("a percentage limit on an action that has none");
            }
            if permission.most_percent.is_some_and(|most| most > MAX_PERCENT) {
                return Err("a percentage limit over 100%");
            }
        }
        Ok(())
    }
}

/// Why a PIN can't be set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum PinError {
    /// It isn't 4 to 8 digits.
    #[error("a PIN is 4 to 8 digits")]
    Malformed,
    /// The hash couldn't be computed: memory couldn't be had.
    #[error("the PIN couldn't be hashed")]
    Hash,
}

/// A PIN, as a profile keeps it: its Argon2id hash, and the salt drawn for it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PinHash {
    salt: [u8; 16],
    hash: [u8; 32],
}

impl fmt::Debug for PinHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Neither the salt nor the hash is a secret, but neither is worth a log line.
        f.write_str("PinHash(..)")
    }
}

/// Whether `pin` is 4 to 8 digits.
fn is_pin(pin: &str) -> bool {
    (MIN_PIN..=MAX_PIN).contains(&pin.len()) && pin.bytes().all(|byte| byte.is_ascii_digit())
}

/// The Argon2id hash of `pin` with `salt`.
fn argon2id(pin: &str, salt: &[u8; 16]) -> Result<[u8; 32], PinError> {
    let params = Params::new(MEMORY, PASSES, LANES, Some(32)).map_err(|_| PinError::Hash)?;
    let mut hash = [0_u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(pin.as_bytes(), salt, &mut hash)
        .map_err(|_| PinError::Hash)?;
    Ok(hash)
}

impl PinHash {
    /// `pin`'s hash with `salt`, which whoever sets the PIN draws.
    ///
    /// # Errors
    /// [`PinError::Malformed`] if `pin` isn't 4 to 8 digits; [`PinError::Hash`] if the memory the
    /// hash needs can't be had.
    pub fn new(pin: &str, salt: [u8; 16]) -> Result<PinHash, PinError> {
        if !is_pin(pin) {
            return Err(PinError::Malformed);
        }
        Ok(PinHash { salt, hash: argon2id(pin, &salt)? })
    }

    /// A PIN's hash from its parts, as a profile holds them.
    pub const fn from_parts(salt: [u8; 16], hash: [u8; 32]) -> PinHash {
        PinHash { salt, hash }
    }

    /// The salt.
    pub const fn salt(&self) -> &[u8; 16] {
        &self.salt
    }

    /// The hash.
    pub const fn hash(&self) -> &[u8; 32] {
        &self.hash
    }

    /// Whether `pin` is the PIN, compared in constant time once hashed. A PIN that isn't 4 to
    /// 8 digits never is.
    pub fn verifies(&self, pin: &str) -> bool {
        if !is_pin(pin) {
            return false;
        }
        argon2id(pin, &self.salt).is_ok_and(|hash| {
            hash.iter().zip(&self.hash).fold(0_u8, |differ, (a, b)| differ | (a ^ b)) == 0
        })
    }
}

/// A count or a limit, as encoded: the role's rules judge its value.
struct Number(u64);

impl Field for Number {
    fn to_value(&self) -> Value {
        Value::Unsigned(self.0)
    }

    fn from_value(value: &Value) -> Option<Number> {
        value.as_u64().map(Number)
    }
}

impl Field for PinHash {
    fn to_value(&self) -> Value {
        Record::default()
            .field(1, &Bytes(self.salt.to_vec()))
            .field(2, &Bytes(self.hash.to_vec()))
            .build()
    }

    fn from_value(value: &Value) -> Option<PinHash> {
        let mut fields = Fields::read(value).ok()?;
        let salt: Bytes = fields.required(1, "salt").ok()?;
        let hash: Bytes = fields.required(2, "hash").ok()?;
        fields.finish().ok()?;
        Some(PinHash { salt: salt.0.try_into().ok()?, hash: hash.0.try_into().ok()? })
    }
}

/// Bytes, as encoded.
struct Bytes(Vec<u8>);

impl Field for Bytes {
    fn to_value(&self) -> Value {
        Value::Bytes(self.0.clone())
    }

    fn from_value(value: &Value) -> Option<Bytes> {
        value.as_bytes().map(|bytes| Bytes(bytes.to_vec()))
    }
}

impl Field for Permission {
    fn to_value(&self) -> Value {
        Record::default()
            .field(1, &self.action)
            .optional(2, self.most_amount.map(Number).as_ref())
            .optional(3, self.most_percent.map(|most| Number(u64::from(most))).as_ref())
            .build()
    }

    fn from_value(value: &Value) -> Option<Permission> {
        let mut fields = Fields::read(value).ok()?;
        let action = fields.required(1, "action").ok()?;
        let most_amount: Option<Number> = fields.optional(2, "most amount").ok()?;
        let most_percent: Option<Number> = fields.optional(3, "most percent").ok()?;
        fields.finish().ok()?;
        Some(Permission {
            action,
            most_amount: most_amount.map(|most| most.0),
            most_percent: match most_percent {
                Some(most) => Some(u32::try_from(most.0).ok()?),
                None => None,
            },
        })
    }
}

impl Field for Role {
    fn to_value(&self) -> Value {
        Record::default()
            .field(1, &self.id)
            .field(2, &self.name)
            .field(3, &self.permissions)
            .build()
    }

    fn from_value(value: &Value) -> Option<Role> {
        let mut fields = Fields::read(value).ok()?;
        let role = Role {
            id: fields.required(1, "role").ok()?,
            name: fields.required(2, "role name").ok()?,
            permissions: fields.required(3, "permissions").ok()?,
        };
        fields.finish().ok()?;
        Some(role)
    }
}

/// An approval: `approver` let the member who asked take `action`, for the amount or
/// percentage it was for, on `subject`, such as the order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Granted {
    approver: Id<TeamMember>,
    action: Action,
    amount: Option<Money>,
    percent: Option<u32>,
    subject: Option<Id<Aggregate>>,
}

impl Granted {
    /// `approver`'s approval of `request` on `subject`.
    ///
    /// # Errors
    /// [`PayloadError::Invalid`] if the request has an amount or a percentage its action has no
    /// limit for, or a percentage over 100%.
    pub fn new(
        approver: Id<TeamMember>,
        request: Request,
        subject: Option<Id<Aggregate>>,
    ) -> Result<Granted, PayloadError> {
        if request.amount.is_some() && !request.action.has_amount() {
            return Err(PayloadError::Invalid("amount"));
        }
        if request
            .percent
            .is_some_and(|percent| !request.action.has_percent() || percent > MAX_PERCENT)
        {
            return Err(PayloadError::Invalid("percent"));
        }
        Ok(Granted {
            approver,
            action: request.action,
            amount: request.amount,
            percent: request.percent,
            subject,
        })
    }

    /// Who approved.
    pub const fn approver(&self) -> Id<TeamMember> {
        self.approver
    }

    /// What they approved.
    pub const fn request(&self) -> Request {
        Request { action: self.action, amount: self.amount, percent: self.percent }
    }

    /// What it was approved on.
    pub const fn subject(&self) -> Option<Id<Aggregate>> {
        self.subject
    }
}

/// What can happen in an approval's stream: it is granted, once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApprovalEvent {
    /// An approval was granted.
    Granted(Granted),
}

impl DomainEvent for ApprovalEvent {
    const STREAM: &'static str = STREAM;

    const SCHEMAS: &'static [SchemaId] = &[GRANTED];

    fn schema(&self) -> SchemaId {
        match self {
            ApprovalEvent::Granted(_) => GRANTED,
        }
    }

    fn to_value(&self) -> Value {
        match self {
            ApprovalEvent::Granted(granted) => Record::default()
                .field(1, &granted.approver)
                .field(2, &granted.action)
                .optional(3, granted.amount.as_ref())
                .optional(4, granted.percent.map(|percent| Number(u64::from(percent))).as_ref())
                .optional(5, granted.subject.as_ref())
                .build(),
        }
    }

    fn from_value(schema: &SchemaRef, payload: &Value) -> Result<ApprovalEvent, DecodeError> {
        if !GRANTED.matches(schema) {
            return Err(DecodeError::UnknownSchema);
        }
        let mut fields = Fields::read(payload)?;
        let approver = fields.required(1, "approver")?;
        let action = fields.required(2, "action")?;
        let amount = fields.optional(3, "amount")?;
        let percent: Option<Number> = fields.optional(4, "percent")?;
        let subject = fields.optional(5, "subject")?;
        fields.finish()?;
        let percent = percent
            .map(|percent| u32::try_from(percent.0).map_err(|_| PayloadError::Invalid("percent")))
            .transpose()?;
        let request = Request { action, amount, percent };
        Ok(ApprovalEvent::Granted(Granted::new(approver, request, subject)?))
    }
}
