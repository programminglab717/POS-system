//! Property tests of kitchen tickets (ADR-0034).
//!
//! - The fold: random histories, from several devices and now and then another location, folded
//!   against a model that reads the decision's rules event by event.
//! - The commands: whatever the ticket's state, a command is accepted exactly when its event,
//!   applied, changes the ticket and reports nothing: a command never records what the fold
//!   would report, nor what changes nothing.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

mod support;

use keel_domain::aggregate::{Aggregate, EventMeta};
use keel_domain::kitchen::{
    ConflictKind, KitchenCommand, KitchenEvent, KitchenTicket, RerouteTo, TicketSent,
};
use keel_domain::order::Mode;
use keel_events::envelope::{Actor, Device, Event, Location};
use keel_types::{Hlc, Id};
use proptest::prelude::*;
use support::{any_kitchen_items, id};

fn location(other: bool) -> Id<Location> {
    id(if other { 0x101 } else { 0x100 })
}

/// An event of a ticket's history, and the device and location that recorded it.
#[derive(Clone, Debug)]
struct Step {
    event: KitchenEvent,
    device: u64,
    elsewhere: bool,
}

fn any_sent() -> impl Strategy<Value = KitchenEvent> {
    any_kitchen_items().prop_map(|items| {
        KitchenEvent::Sent(TicketSent {
            order: id(0x800),
            station: id(0x701),
            mode: Mode::DineIn,
            items,
        })
    })
}

fn any_step() -> impl Strategy<Value = Step> {
    let event = prop_oneof![
        1 => any_sent(),
        2 => Just(KitchenEvent::Shown),
        3 => Just(KitchenEvent::Bumped),
        2 => Just(KitchenEvent::Recalled),
        1 => prop::sample::select(RerouteTo::ALL.to_vec()).prop_map(KitchenEvent::Rerouted),
    ];
    (event, 1_u64..=3, prop::bool::weighted(0.1)).prop_map(|(event, device, elsewhere)| Step {
        event,
        device,
        elsewhere,
    })
}

/// A history, sent first most of the time, as a fire writes it.
fn any_history() -> impl Strategy<Value = Vec<Step>> {
    (prop::bool::weighted(0.85), any_sent(), prop::collection::vec(any_step(), 0..=12)).prop_map(
        |(first, sent, mut steps)| {
            if first {
                steps.insert(0, Step { event: sent, device: 1, elsewhere: false });
            }
            steps
        },
    )
}

fn meta(at: usize, step: &Step) -> EventMeta {
    let n = u64::try_from(at).unwrap() + 1;
    EventMeta {
        event_id: id::<Event>(0xE000 + n),
        location: location(step.elsewhere),
        origin_device: id::<Device>(0xD0 + step.device),
        origin_seq: n.try_into().unwrap(),
        hlc: Hlc::new(1_794_560_400_000 + n, 0).unwrap(),
        business_date: "2026-11-13".parse().unwrap(),
        actor: Actor::TeamMember(id(0x300)),
        approval: None,
    }
}

fn fold(history: &[Step]) -> KitchenTicket {
    let mut ticket = KitchenTicket::new(id(0xA00));
    for (at, step) in history.iter().enumerate() {
        ticket.apply(&meta(at, step), &step.event);
    }
    ticket
}

/// What the model says of a ticket: its sending, first showing, latest bump or recall, and first
/// reroute, each by the position of its event; and the conflicts, by position.
#[derive(Debug, Default, PartialEq, Eq)]
struct Model {
    sent: Option<(usize, bool)>,
    shown: Option<usize>,
    bump: Option<(bool, usize)>,
    rerouted: Option<(RerouteTo, usize)>,
    conflicts: Vec<(usize, ConflictKind)>,
}

fn model(history: &[Step]) -> Model {
    let mut model = Model::default();
    for (at, step) in history.iter().enumerate() {
        // After it is sent, an event elsewhere than where it was sent doesn't apply.
        if let Some((_, elsewhere)) = model.sent
            && elsewhere != step.elsewhere
        {
            model.conflicts.push((at, ConflictKind::WrongLocation));
            continue;
        }
        match &step.event {
            KitchenEvent::Sent(_) if model.sent.is_some() => {
                model.conflicts.push((at, ConflictKind::SentTwice));
            }
            KitchenEvent::Sent(_) => model.sent = Some((at, step.elsewhere)),
            _ if model.sent.is_none() => model.conflicts.push((at, ConflictKind::BeforeSent)),
            KitchenEvent::Shown => {
                if model.shown.is_none() {
                    model.shown = Some(at);
                    if model.rerouted.is_some() {
                        model.conflicts.push((at, ConflictKind::ShownAndRerouted));
                    }
                }
            }
            KitchenEvent::Bumped => model.bump = Some((true, at)),
            KitchenEvent::Recalled => model.bump = Some((false, at)),
            KitchenEvent::Rerouted(to) => {
                if model.rerouted.is_none() {
                    model.rerouted = Some((*to, at));
                    if model.shown.is_some() {
                        model.conflicts.push((at, ConflictKind::ShownAndRerouted));
                    }
                }
            }
        }
    }
    model
}

/// The ticket as the model sees it, from the fold.
fn observed(ticket: &KitchenTicket, history: &[Step]) -> Model {
    let position = |event: Id<Event>| {
        (0..history.len()).find(|&at| meta(at, &history[at]).event_id == event).unwrap()
    };
    Model {
        sent: ticket.sent().map(|(_, moment)| {
            let at = position(moment.event);
            (at, history[at].elsewhere)
        }),
        shown: ticket.shown().map(|moment| position(moment.event)),
        bump: ticket.last_bump_or_recall().map(|(bumped, moment)| (bumped, position(moment.event))),
        rerouted: ticket.rerouted().map(|(to, moment)| (to, position(moment.event))),
        conflicts: ticket
            .conflicts()
            .iter()
            .map(|conflict| (position(conflict.event), conflict.kind))
            .collect(),
    }
}

/// What a ticket shows of itself, to tell whether an event changed it.
fn state(ticket: &KitchenTicket) -> (bool, bool, bool, Option<RerouteTo>) {
    (
        ticket.sent().is_some(),
        ticket.shown().is_some(),
        ticket.is_bumped(),
        ticket.rerouted().map(|(to, _)| to),
    )
}

fn command_of(event: &KitchenEvent) -> KitchenCommand {
    match event {
        KitchenEvent::Sent(sent) => KitchenCommand::Send(sent.clone()),
        KitchenEvent::Shown => KitchenCommand::Show,
        KitchenEvent::Bumped => KitchenCommand::Bump,
        KitchenEvent::Recalled => KitchenCommand::Recall,
        KitchenEvent::Rerouted(to) => KitchenCommand::Reroute(*to),
    }
}

proptest! {
    /// The fold agrees with the model, whatever the history.
    #[test]
    fn tickets_fold_as_the_model_says(history in any_history()) {
        let ticket = fold(&history);
        prop_assert_eq!(observed(&ticket, &history), model(&history));
        prop_assert_eq!(ticket.is_bumped(), model(&history).bump.is_some_and(|(bumped, _)| bumped));
    }

    /// A command is accepted exactly when its event, applied, changes the ticket and reports
    /// nothing; and the event it records is the command's.
    #[test]
    fn commands_record_only_what_changes_the_ticket_and_fits(
        history in any_history(),
        next in any_step(),
    ) {
        let ticket = fold(&history);
        let mut extended = history.clone();
        extended.push(next.clone());
        let after = fold(&extended);
        let fits = after.conflicts().len() == ticket.conflicts().len();
        let changes = state(&after) != state(&ticket)
            || after.sent().map(|(sent, _)| sent.clone()) != ticket.sent().map(|(sent, _)| sent.clone());
        match ticket.decide(location(next.elsewhere), command_of(&next.event)) {
            Ok(event) => {
                prop_assert_eq!(&event, &next.event);
                prop_assert!(fits && changes, "recorded {event:?}: fits {fits}, changes {changes}");
            }
            Err(error) => prop_assert!(!(fits && changes), "refused {:?}: {error}", next.event),
        }
    }
}
