//! Known answers for hub terms: the epoch, each rule a claim must keep, reading payloads
//! strictly, the pinned payload, which claim wins, the chain and cuts that claims make as they
//! arrive in any order, and when a replica may claim.

use keel_events::cbor::{Map, Value};
use keel_events::envelope::{SchemaName, SchemaRef};

use super::*;
use crate::codec::PayloadError;
use crate::schema::{DecodeError, DomainEvent};

/// One past the largest epoch or position.
const PAST_MAX: u64 = MAX_NUMBER + 1;

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

fn device(n: u64) -> Id<Device> {
    id(n)
}

fn epoch(n: u64) -> Epoch {
    Epoch::new(n).unwrap()
}

fn priority(n: u8) -> NonZeroU8 {
    NonZeroU8::new(n).unwrap()
}

/// Cuts of devices `n`, each at its position, in the order given.
fn cuts(of: &[(u64, u64)]) -> Vec<Cut> {
    of.iter().map(|&(n, position)| Cut { device: device(n), position }).collect()
}

/// What a claim succeeds: claim `previous`, with cuts `of`.
fn succession(previous: u64, of: &[(u64, u64)]) -> Succession {
    Succession { previous: id(previous), cuts: cuts(of) }
}

fn invalid(rule: &'static str) -> Result<Claimed, PayloadError> {
    Err(PayloadError::Invalid(rule))
}

/// Decodes the payload of a claim built without its rules being checked.
fn decode(claimed: Claimed) -> Result<HubEvent, DecodeError> {
    HubEvent::from_value(&CLAIMED.to_ref().unwrap(), &HubEvent::Claimed(claimed).to_value())
}

/// The claim recorded by event `event`, at `position` of device `n`'s log.
fn claim(event: u64, n: u64, position: u64, claimed: Claimed) -> Claim {
    Claim { event: id(event), device: device(n), position, claimed }
}

/// A first claim, of epoch 1.
fn first(event: u64, n: u64, position: u64, of_priority: u8) -> Claim {
    let claimed = Claimed::new(Epoch::FIRST, priority(of_priority), None).unwrap();
    claim(event, n, position, claimed)
}

/// A claim of `of_epoch` that succeeds `succeeds`.
fn later(
    event: u64,
    n: u64,
    position: u64,
    of_epoch: u64,
    of_priority: u8,
    succeeds: Succession,
) -> Claim {
    let claimed = Claimed::new(epoch(of_epoch), priority(of_priority), Some(succeeds)).unwrap();
    claim(event, n, position, claimed)
}

/// A term: epoch `of_epoch`, held by device `n` from claim `event` at `after`, cut at `through`.
fn term(of_epoch: u64, n: u64, event: u64, after: u64, through: Option<u64>) -> Term {
    Term { epoch: epoch(of_epoch), device: device(n), claim: id(event), after, through }
}

/// Every order of `claims`.
fn orders(claims: &[Claim]) -> Vec<Vec<Claim>> {
    if claims.len() <= 1 {
        return vec![claims.to_vec()];
    }
    let mut all = Vec::new();
    for (index, claim) in claims.iter().enumerate() {
        let mut rest = claims.to_vec();
        rest.remove(index);
        for mut order in orders(&rest) {
            order.insert(0, claim.clone());
            all.push(order);
        }
    }
    all
}

/// Checks that `claims`, in every order they could arrive in, make `expected`.
fn makes(claims: &[Claim], expected: &[Term]) {
    for order in orders(claims) {
        assert_eq!(chain(&order), expected, "{order:?}");
    }
}

#[test]
fn epochs_are_from_1_to_the_largest_the_store_holds() {
    assert_eq!(Epoch::new(0), None);
    assert_eq!(Epoch::new(1), Some(Epoch::FIRST));
    assert_eq!(Epoch::new(MAX_NUMBER), Some(Epoch::MAX));
    assert_eq!(Epoch::new(PAST_MAX), None);
    assert_eq!(Epoch::new(u64::MAX), None);
    assert_eq!(Epoch::FIRST.get(), 1);
    assert_eq!(Epoch::MAX.get(), MAX_NUMBER);
    assert_eq!(Epoch::FIRST.next(), Some(epoch(2)));
    assert_eq!(epoch(MAX_NUMBER - 1).next(), Some(Epoch::MAX));
    assert_eq!(Epoch::MAX.next(), None);
    assert_eq!(Epoch::from_value(&Value::Unsigned(0)), None);
    assert_eq!(Epoch::from_value(&Value::Unsigned(PAST_MAX)), None);
    assert_eq!(Epoch::from_value(&Value::integer(-1)), None);
    assert_eq!(Epoch::from_value(&Epoch::MAX.to_value()), Some(Epoch::MAX));
}

#[test]
fn a_claim_of_epoch_1_succeeds_none_and_a_later_one_succeeds_one() {
    let first = Claimed::new(Epoch::FIRST, priority(2), None).unwrap();
    assert_eq!(first.succeeds, None);
    assert_eq!(Claimed::new(epoch(2), priority(2), None), invalid("previous"));
    assert_eq!(Claimed::new(Epoch::MAX, priority(2), None), invalid("previous"));
    let succeeds = succession(0xE1, &[(1, 5)]);
    assert_eq!(
        Claimed::new(Epoch::FIRST, priority(2), Some(succeeds.clone())),
        invalid("previous")
    );
    for of_epoch in [2, 3, MAX_NUMBER] {
        let later = Claimed::new(epoch(of_epoch), priority(1), Some(succeeds.clone())).unwrap();
        assert_eq!(later.succeeds.as_ref(), Some(&succeeds));
    }
}

#[test]
fn a_claim_cuts_each_device_once_in_order_from_position_1() {
    let with = |of: &[(u64, u64)]| {
        Claimed::new(epoch(4), priority(1), Some(succession(0xE1, of))).map(|_| ())
    };
    assert_eq!(with(&[(1, 1)]), Ok(()));
    assert_eq!(with(&[(1, MAX_NUMBER), (2, 1), (0xFF, 9)]), Ok(()));
    assert_eq!(with(&[]), Err(PayloadError::Invalid("cuts")));
    for position in [0, PAST_MAX, u64::MAX] {
        assert_eq!(with(&[(1, position)]), Err(PayloadError::Invalid("cut")), "{position}");
        assert_eq!(with(&[(1, 3), (2, position)]), Err(PayloadError::Invalid("cut")), "{position}");
    }
    for out_of_order in [&[(2, 3), (1, 3)][..], &[(1, 3), (1, 4)], &[(1, 3), (3, 3), (2, 3)]] {
        assert_eq!(
            with(out_of_order),
            Err(PayloadError::Invalid("cuts out of order")),
            "{out_of_order:?}"
        );
    }
}

#[test]
fn a_claim_holds_at_most_the_most_cuts() {
    let devices = |count: usize| -> Vec<Cut> {
        (1..=u64::try_from(count).unwrap())
            .map(|n| Cut { device: device(n), position: n })
            .collect()
    };
    let most = Succession { previous: id(0xE1), cuts: devices(MAX_CUTS) };
    assert!(Claimed::new(epoch(2), priority(1), Some(most)).is_ok());
    let too_many = Succession { previous: id(0xE1), cuts: devices(MAX_CUTS + 1) };
    assert_eq!(Claimed::new(epoch(2), priority(1), Some(too_many)), invalid("cuts"));
}

#[test]
fn decoding_keeps_the_same_rules() {
    let refused = |claimed: Claimed, rule| {
        assert_eq!(
            decode(claimed),
            Err(DecodeError::Malformed(PayloadError::Invalid(rule))),
            "{rule}"
        );
    };
    let unchecked = |of_epoch: u64, succeeds: Option<Succession>| Claimed {
        epoch: Epoch(of_epoch),
        priority: priority(3),
        succeeds,
    };
    refused(unchecked(0, None), "epoch");
    refused(unchecked(PAST_MAX, None), "epoch");
    refused(unchecked(2, None), "previous");
    refused(unchecked(1, Some(succession(0xE1, &[(1, 5)]))), "previous");
    refused(unchecked(3, Some(succession(0xE1, &[]))), "cuts");
    refused(unchecked(3, Some(succession(0xE1, &[(1, 0)]))), "cut");
    refused(unchecked(3, Some(succession(0xE1, &[(2, 5), (1, 5)]))), "cuts out of order");
    for claimed in [unchecked(1, None), unchecked(3, Some(succession(0xE1, &[(1, 5), (2, 1)])))] {
        assert_eq!(decode(claimed.clone()), Ok(HubEvent::Claimed(claimed)));
    }
}

#[test]
fn payloads_are_read_strictly() {
    let claimed =
        Claimed::new(epoch(3), priority(2), Some(succession(0xE1, &[(1, 5), (2, 7)]))).unwrap();
    let payload = HubEvent::Claimed(claimed).to_value().as_map().unwrap().clone().into_entries();
    let schema = CLAIMED.to_ref().unwrap();
    let with = |changes: &[(u64, Option<Value>)]| {
        let mut entries = payload.clone();
        for (key, value) in changes {
            entries.retain(|(existing, _)| existing.as_u64() != Some(*key));
            entries.extend(value.clone().map(|value| (Value::Unsigned(*key), value)));
        }
        HubEvent::from_value(&schema, &Value::Map(Map::from_entries(entries).unwrap()))
    };
    let malformed = |error| Err(DecodeError::Malformed(error));
    assert_eq!(
        with(&[(5, Some(Value::Unsigned(1)))]),
        malformed(PayloadError::UnknownField("5".into()))
    );
    assert_eq!(with(&[(1, None)]), malformed(PayloadError::Missing("epoch")));
    assert_eq!(with(&[(2, None)]), malformed(PayloadError::Missing("priority")));
    // A claim that succeeds one cuts something; one that succeeds none cuts nothing.
    assert_eq!(with(&[(4, None)]), malformed(PayloadError::Missing("cuts")));
    assert_eq!(with(&[(3, None)]), malformed(PayloadError::Invalid("cuts")));
    assert_eq!(
        with(&[(1, Some(Value::Unsigned(1))), (3, None), (4, None)]),
        Ok(HubEvent::Claimed(Claimed::new(Epoch::FIRST, priority(2), None).unwrap()))
    );
    let cut =
        |device: Value, position: Value| Value::Array(vec![Value::Array(vec![device, position])]);
    let wrong = [
        (1, Value::from("3"), "epoch"),
        (1, Value::Unsigned(0), "epoch"),
        (1, Value::Unsigned(PAST_MAX), "epoch"),
        (2, Value::Unsigned(0), "priority"),
        (2, Value::Unsigned(256), "priority"),
        (2, Value::integer(-1), "priority"),
        (3, Value::Bytes(vec![0; 16]), "previous"),
        (3, Value::Bytes(id::<Event>(0xE1).to_bytes()[..15].to_vec()), "previous"),
        (3, Value::Null, "previous"),
        (4, Value::Map(Map::new()), "cuts"),
        (4, Value::Null, "cuts"),
        (4, Value::Array(vec![Value::Array(vec![device(1).to_value()])]), "cuts"),
        (
            4,
            Value::Array(vec![Value::Array(vec![
                device(1).to_value(),
                Value::Unsigned(5),
                Value::Null,
            ])]),
            "cuts",
        ),
        (4, cut(device(1).to_value(), Value::integer(-5)), "cuts"),
        (4, cut(Value::Bytes(vec![0; 16]), Value::Unsigned(5)), "cuts"),
    ];
    for (key, value, rule) in wrong {
        assert_eq!(
            with(&[(key, Some(value.clone()))]),
            malformed(PayloadError::Invalid(rule)),
            "{key}: {value:?}"
        );
    }
    assert_eq!(
        HubEvent::from_value(&schema, &Value::Array(Vec::new())),
        malformed(PayloadError::NotAMap)
    );
}

#[test]
fn the_registry_lists_the_schema_once() {
    assert_eq!(HubEvent::SCHEMAS, &[CLAIMED]);
    assert_eq!(HubEvent::STREAM, "hub");
    let reference = CLAIMED.to_ref().unwrap();
    assert_eq!(reference.name.as_str(), "hub.claimed");
    assert!(CLAIMED.matches(&reference));
    let claimed = Claimed::new(Epoch::FIRST, priority(1), None).unwrap();
    assert_eq!(HubEvent::Claimed(claimed.clone()).schema(), CLAIMED);
    let payload = HubEvent::Claimed(claimed).to_value();
    let unknown = [("hub.resigned", 1_u32), ("hub.claimed", 2), ("sequence.assigned", 1)];
    for (name, version) in unknown {
        let schema = SchemaRef {
            name: SchemaName::new(name).unwrap(),
            version: version.try_into().unwrap(),
        };
        assert_eq!(HubEvent::from_value(&schema, &payload), Err(DecodeError::UnknownSchema));
    }
}

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The claim's payload, pinned forever: a first claim, a claim that succeeds one, and one at the
/// largest numbers. Python's `cbor2` encoded each one itself from the documented key table, and
/// confirmed it is canonical. If this test fails, the payload format changed, and stored claims
/// would no longer decode.
#[test]
fn the_payload_format_is_pinned() {
    let pinned = [
        (Claimed { epoch: Epoch::FIRST, priority: priority(2), succeeds: None }, "a201010202"),
        (
            Claimed {
                epoch: epoch(3),
                priority: priority(1),
                succeeds: Some(Succession {
                    previous: id(0xE2),
                    cuts: vec![
                        Cut { device: device(1), position: 40 },
                        Cut { device: device(2), position: 7 },
                    ],
                }),
            },
            "a40103020103500192f0c10000700080000000000000e2048282500192f0c1000070008000000000000001182882500192f0c100007000800000000000000207",
        ),
        (
            Claimed {
                epoch: Epoch::MAX,
                priority: priority(255),
                succeeds: Some(Succession {
                    previous: id(0xFFFF_FFFF_FFFF),
                    cuts: vec![Cut { device: device(0xFFFF_FFFF_FFFF), position: MAX_NUMBER }],
                }),
            },
            "a4011b7fffffffffffffff0218ff03500192f0c1000070008000ffffffffffff048182500192f0c1000070008000ffffffffffff1b7fffffffffffffff",
        ),
    ];
    for (claimed, hex) in pinned {
        let event = HubEvent::Claimed(claimed);
        let (schema, payload) = event.encode().unwrap();
        assert_eq!(schema.name.as_str(), "hub.claimed");
        assert_eq!(payload.as_bytes(), bytes(hex).as_slice());
        assert_eq!(HubEvent::decode(&schema, &payload), Ok(event));
    }
}

#[test]
fn the_highest_epoch_wins_then_priority_then_the_lower_device_then_the_earlier_claim() {
    let base = later(0xE1, 5, 9, 4, 2, succession(0xE0, &[(1, 5)]));
    let with = |n: u64, position: u64, of_epoch: u64, of_priority: u8| {
        later(0xE2, n, position, of_epoch, of_priority, succession(0xE0, &[(1, 5)]))
    };
    // Each of these beats `base`, and `base` beats each of the next.
    let better = [
        with(9, 9, 5, 1),
        with(5, 9, 4, 3),
        with(9, 1, 4, 255),
        with(4, 9, 4, 2),
        with(5, 8, 4, 2),
    ];
    let worse = [with(1, 1, 3, 255), with(5, 9, 4, 1), with(6, 9, 4, 2), with(5, 10, 4, 2)];
    for claim in &better {
        assert!(claim.beats(&base) && !base.beats(claim), "{claim:?}");
    }
    for claim in &worse {
        assert!(base.beats(claim) && !claim.beats(&base), "{claim:?}");
    }
    assert!(!base.beats(&base));
    // The epoch counts above all: a first claim of the highest priority loses to any later one.
    let highest = first(0xE3, 1, 1, 255);
    assert!(base.beats(&highest));
}

#[test]
fn no_claims_make_no_chain_and_a_first_claim_a_term_no_one_cuts() {
    assert_eq!(chain(&[]), []);
    let hub = first(0xE1, 1, 10, 2);
    assert_eq!(chain(&[hub]), [term(1, 1, 0xE1, 10, None)]);
    let only = term(1, 1, 0xE1, 10, None);
    assert!(!only.counts(9) && !only.counts(10));
    assert!(only.counts(11) && only.counts(MAX_NUMBER));
}

#[test]
fn a_successor_cuts_the_hub_it_succeeds_where_it_held_its_log() {
    // The hub, device 1, claims at position 10; the standby, device 2, holds its log to 15 when
    // it claims, at position 3 of its own.
    let hub = first(0xE1, 1, 10, 2);
    let standby = later(0xE2, 2, 3, 2, 1, succession(0xE1, &[(1, 15)]));
    let expected = [term(2, 2, 0xE2, 3, None), term(1, 1, 0xE1, 10, Some(15))];
    makes(&[hub, standby], &expected);
    let deposed = expected[1];
    assert!(!deposed.counts(10) && deposed.counts(11) && deposed.counts(15));
    assert!(!deposed.counts(16) && !deposed.counts(MAX_NUMBER));
}

#[test]
fn of_two_claims_to_succeed_one_hub_one_wins_and_the_other_term_counts_nothing() {
    // A split brain: devices 2 and 3 each claim epoch 2 after the hub, each on its side.
    let hub = first(0xE1, 1, 10, 3);
    let left = later(0xE2, 2, 4, 2, 2, succession(0xE1, &[(1, 15)]));
    let right = later(0xE3, 3, 6, 2, 1, succession(0xE1, &[(1, 12)]));
    let left_wins = [term(2, 2, 0xE2, 4, None), term(1, 1, 0xE1, 10, Some(15))];
    makes(&[hub.clone(), left.clone(), right], &left_wins);
    // Of one priority, the lower device wins.
    let right = later(0xE3, 3, 6, 2, 2, succession(0xE1, &[(1, 12)]));
    makes(&[hub.clone(), left.clone(), right.clone()], &left_wins);
    // The side that claims again wins, whatever the priorities: and the chain runs through its
    // own claims.
    let again = later(0xE4, 4, 2, 3, 1, succession(0xE3, &[(1, 12), (3, 9)]));
    makes(
        &[hub, left, right, again],
        &[term(3, 4, 0xE4, 2, None), term(2, 3, 0xE3, 6, Some(9)), term(1, 1, 0xE1, 10, Some(12))],
    );
}

#[test]
fn a_hub_elected_twice_is_cut_by_every_claim_after_each_of_its_terms() {
    // Device 1, then 2, then 1 again, then 2 again.
    let claims = [
        first(0xE1, 1, 10, 2),
        later(0xE2, 2, 3, 2, 1, succession(0xE1, &[(1, 15)])),
        later(0xE3, 1, 30, 3, 2, succession(0xE2, &[(1, 29), (2, 8)])),
        later(0xE4, 2, 12, 4, 1, succession(0xE3, &[(1, 41), (2, 11)])),
    ];
    makes(
        &claims,
        &[
            term(4, 2, 0xE4, 12, None),
            term(3, 1, 0xE3, 30, Some(41)),
            term(2, 2, 0xE2, 3, Some(8)),
            term(1, 1, 0xE1, 10, Some(15)),
        ],
    );
    // A later claim that held less of a log than an earlier one cuts it shorter.
    let shorter = later(0xE4, 2, 12, 4, 1, succession(0xE3, &[(1, 41), (2, 6)]));
    let claims = [claims[0].clone(), claims[1].clone(), claims[2].clone(), shorter];
    assert_eq!(chain(&claims)[2], term(2, 2, 0xE2, 3, Some(6)));
}

#[test]
fn a_later_claim_that_cuts_no_part_of_a_device_held_none_of_its_log() {
    let claims = [
        first(0xE1, 1, 10, 2),
        later(0xE2, 2, 3, 2, 1, succession(0xE1, &[(1, 15)])),
        // It cuts device 2, but not device 1.
        later(0xE3, 3, 5, 3, 1, succession(0xE2, &[(2, 8)])),
    ];
    makes(
        &claims,
        &[term(3, 3, 0xE3, 5, None), term(2, 2, 0xE2, 3, Some(8)), term(1, 1, 0xE1, 10, Some(0))],
    );
    let uncut = chain(&claims)[2];
    assert!(!(0..=100).any(|position| uncut.counts(position)));
}

#[test]
fn the_chain_ends_where_the_replica_holds_no_more_of_it() {
    let hub = first(0xE1, 1, 10, 2);
    let standby = later(0xE2, 2, 3, 2, 1, succession(0xE1, &[(1, 15)]));
    let third = later(0xE3, 3, 5, 3, 1, succession(0xE2, &[(1, 15), (2, 8)]));
    // Without the standby's claim, the chain stops at the third's.
    makes(&[hub.clone(), third.clone()], &[term(3, 3, 0xE3, 5, None)]);
    makes(core::slice::from_ref(&third), &[term(3, 3, 0xE3, 5, None)]);
    // Without the hub's, it stops at the standby's, cut by the third's claim.
    makes(
        &[standby.clone(), third.clone()],
        &[term(3, 3, 0xE3, 5, None), term(2, 2, 0xE2, 3, Some(8))],
    );
    // A claim that names one of its own epoch or a later one as the claim it succeeds has none
    // before it on the chain.
    let rival = later(0xE4, 4, 2, 3, 1, succession(0xE1, &[(1, 15)]));
    let looped = later(0xE5, 5, 2, 3, 2, succession(0xE4, &[(4, 2)]));
    makes(&[hub.clone(), rival.clone(), looped], &[term(3, 5, 0xE5, 2, None)]);
    let itself = later(0xE6, 6, 2, 4, 1, succession(0xE6, &[(6, 1)]));
    makes(&[hub, rival, itself], &[term(4, 6, 0xE6, 2, None)]);
}

#[test]
fn a_successor_claims_the_next_epoch_and_cuts_each_device_on_its_chain() {
    let held = |of: Id<Device>| if of == device(1) { 41 } else { 11 };
    let first_claim = Claimed::succeeding(&[], priority(2), held).unwrap();
    assert_eq!(first_claim, Claimed::new(Epoch::FIRST, priority(2), None).unwrap());
    let whole =
        [term(3, 1, 0xE3, 30, None), term(2, 2, 0xE2, 3, Some(11)), term(1, 1, 0xE1, 10, Some(15))];
    let claimed = Claimed::succeeding(&whole, priority(7), held).unwrap();
    assert_eq!(
        claimed,
        Claimed::new(epoch(4), priority(7), Some(succession(0xE3, &[(1, 41), (2, 11)]))).unwrap()
    );
    // The cuts are in order of device, whatever order the chain's devices come in.
    let unordered = [term(2, 9, 0xE2, 1, None), term(1, 3, 0xE1, 1, Some(2))];
    let claimed = Claimed::succeeding(&unordered, priority(1), |_| 4).unwrap();
    assert_eq!(claimed.succeeds.unwrap().cuts, cuts(&[(3, 4), (9, 4)]));
}

#[test]
fn a_replica_claims_only_holding_the_whole_chain_and_every_record_that_counts() {
    let whole =
        [term(3, 1, 0xE3, 30, None), term(2, 2, 0xE2, 3, Some(11)), term(1, 1, 0xE1, 10, Some(15))];
    // Holding each claim, device 2's log to its cut, and device 1's to its first term's will do;
    // the winning claim's term isn't cut yet, so any of its log past the claim will.
    for (one, two) in [(30, 11), (41, 11), (30, 400)] {
        let held = |of: Id<Device>| if of == device(1) { one } else { two };
        assert!(Claimed::succeeding(&whole, priority(1), held).is_ok(), "{one}, {two}");
    }
    // Short of a cut, the replica is behind: its claim would cut records that count. Short of a
    // claim, it can't have worked out the chain at all.
    for (one, two) in [(29, 11), (41, 10), (41, 2), (0, 0)] {
        let held = |of: Id<Device>| if of == device(1) { one } else { two };
        assert_eq!(Claimed::succeeding(&whole, priority(1), held), Err(ClaimError::Behind));
    }
    // A chain that doesn't reach a claim of epoch 1 isn't whole.
    for broken in [&whole[..1], &whole[..2], &whole[1..2]] {
        assert_eq!(Claimed::succeeding(broken, priority(1), |_| 50), Err(ClaimError::Behind));
    }
}

#[test]
fn a_claim_that_would_break_a_rule_is_refused() {
    // When the epochs run out.
    let last = [term(MAX_NUMBER, 1, 0xE2, 1, None), term(1, 2, 0xE1, 1, Some(1))];
    assert_eq!(
        Claimed::succeeding(&last, priority(1), |_| 1),
        Err(ClaimError::Invalid(PayloadError::Invalid("epoch")))
    );
    // More devices than a claim can cut.
    let crowded: Vec<Term> = (1..=u64::try_from(MAX_CUTS + 1).unwrap())
        .rev()
        .map(|n| term(n, n, n, 1, (n <= u64::try_from(MAX_CUTS).unwrap()).then_some(1)))
        .collect();
    assert_eq!(
        Claimed::succeeding(&crowded, priority(1), |_| 1),
        Err(ClaimError::Invalid(PayloadError::Invalid("cuts")))
    );
}
