//! Answering requests for orders (ADR-0021): the Store Hub's grants and refusals.
//!
//! The orders projection keeps, for each order, the requests for it that no answer names yet,
//! and indexes the orders that have any. The hub, the store that holds the winning claim
//! (ADR-0022), answers them all in a write of its own, in its epoch, order by order: it folds the
//! order, sees whether a payment of the order is in progress, and lets the order's rules
//! ([`Order::answers`]) answer each request in canonical order. Each answer is an event on the
//! order's stream, recorded by the hub, caused by the request it answers.

use keel_domain::order::{Epoch, Order, OrderEvent};
use keel_domain::schema::DomainEvent;
use keel_events::envelope::{Actor, Component, StreamKind, StreamRef};
use keel_events::event::SignedEvent;
use keel_events::keys::Signer;
use keel_events::log::EventDraft;
use keel_types::{BusinessDate, Entropy, Id, Timestamp};
use rusqlite::{Connection, OptionalExtension};

use crate::error::StoreError;
use crate::projection;
use crate::schema::id;
use crate::store::noted;
use crate::terms;
use crate::write::Writing;

/// Why an answer can't be recorded: only an epoch out of range can make it so.
fn unrecordable<T>(_: T) -> StoreError {
    StoreError::OutOfRange("an answer to a request")
}

impl<S: Signer, E: Entropy> Writing<'_, S, E> {
    /// Answers, as the hub, every request for an order the store holds that no answer names yet,
    /// at physical time `now`, and returns the answers: none when no request waits.
    pub(crate) fn answer_requests(
        &mut self,
        now: Timestamp,
    ) -> Result<Vec<SignedEvent>, StoreError> {
        let health = self.health;
        let own = self.writer.device();
        let (epoch, waiting) = terms::own_epoch(self.tx, own)
            .and_then(|epoch| Ok((epoch, waiting(self.tx)?)))
            .map_err(|error| noted(health, error))?;
        let mut written = Vec::new();
        for order_id in waiting {
            let answered =
                answers(self.tx, order_id, epoch).map_err(|error| noted(health, error))?;
            let Some((answers, business_date)) = answered else { continue };
            for answer in answers {
                // The request is the answer's cause.
                let causation = match &answer {
                    OrderEvent::OwnershipGranted(granted) => Some(granted.request.cast()),
                    OrderEvent::OwnershipRefused { request, .. } => Some(request.cast()),
                    _ => None,
                };
                let (schema, payload) = answer.encode().map_err(unrecordable)?;
                let draft = EventDraft {
                    stream: StreamRef {
                        kind: StreamKind::new(OrderEvent::STREAM).map_err(unrecordable)?,
                        id: order_id.cast(),
                    },
                    schema,
                    business_date,
                    actor: Actor::System(Component::new("hub").map_err(unrecordable)?),
                    approval: None,
                    causation,
                    correlation: None,
                    payload,
                };
                written.push(self.append(draft, now)?);
            }
        }
        Ok(written)
    }
}

/// The orders with requests waiting, by identifier.
fn waiting(db: &Connection) -> Result<Vec<Id<Order>>, StoreError> {
    let mut statement =
        db.prepare("SELECT order_id FROM orders WHERE requests > 0 ORDER BY order_id")?;
    let rows = statement.query_map([], |row| row.get::<_, Vec<u8>>(0))?;
    rows.map(|row| id(&row?)).collect()
}

/// The hub's answers to the requests for order `order_id`, and the business date to record
/// them under: the order's own. `None` while the store doesn't hold the order's creation.
fn answers(
    db: &Connection,
    order_id: Id<Order>,
    epoch: Epoch,
) -> Result<Option<(Vec<OrderEvent>, BusinessDate)>, StoreError> {
    let key = order_id.to_bytes();
    let order = projection::load(db, Order::new(order_id))?;
    // A payment of the order in flight: initiated, with no outcome yet.
    let paying = db
        .query_row(
            "SELECT 1 FROM payments WHERE order_id = ?1 AND state = 'initiated' LIMIT 1",
            [&key[..]],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let date: Option<String> = db
        .query_row("SELECT business_date FROM orders WHERE order_id = ?1", [&key[..]], |row| {
            row.get(0)
        })
        .optional()?
        .flatten();
    let Some(date) = date else { return Ok(None) };
    let business_date =
        date.parse().map_err(|_| StoreError::Corrupt("an order's business date"))?;
    Ok(Some((order.answers(paying, epoch), business_date)))
}
