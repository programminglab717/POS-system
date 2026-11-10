//! The register's latency with a large catalog (ADR-0023, decision 8; ADR-0027, part 5): the demo
//! café's profile with `KEEL_SKUS` more items (100,000 by default), each with a variant and a
//! price, checked, encoded and decoded as a device receives it; a runtime opened over it; and 500
//! sales rung and paid, eleven open at a time, as slice 1's baseline was measured. It prints each
//! step's time and each intent's median and 99th percentile, and fails only past bounds far
//! beyond what a register may take, so it runs on slow emulators as on the host:
//!
//! ```sh
//! cargo test --release -p keel-runtime --test latency -- --ignored --nocapture
//! ```

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::print_stdout,
    reason = "a harness: it reports to its terminal"
)]

use core::num::NonZeroU32;
use core::time::Duration;
use std::path::PathBuf;
use std::time::Instant;

use keel_domain::codec::Name;
use keel_domain::order::{Mode, Order};
use keel_domain::profile::{CatalogItem, CatalogVariant, Page, Profile, demo};
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_runtime::{Identity, Runtime};
use keel_store::StoreKey;
use keel_types::{Currency, Id, Locale, ManualClock, Money, SeededEntropy};

fn id<T>(n: u64) -> Id<T> {
    Id::parse(&format!("0192f0c1-0000-7000-8000-{n:012x}")).unwrap()
}

/// The first of the large catalog's identifiers: its items, and its variants after them.
const ITEMS: u64 = 0x100_0000;
const VARIANTS: u64 = 0x200_0000;

/// The demo café's profile with `skus` more items, and a page of the first 500 of them.
fn large(skus: u64) -> Profile {
    let mut data = demo().unwrap().data().clone();
    let food = data.catalog.items[0].tax_category;
    for n in 0..skus {
        data.catalog.items.push(CatalogItem {
            id: id(ITEMS + n),
            name: Name::new(&format!("Item {n}")).unwrap(),
            tax_category: food,
            variants: vec![CatalogVariant {
                id: id(VARIANTS + n),
                name: None,
                price: Money::from_minor(100 + i64::try_from(n % 900).unwrap(), Currency::USD),
            }],
            groups: Vec::new(),
        });
    }
    data.menu.pages.push(Page {
        name: Name::new("Many").unwrap(),
        buttons: (0..skus.min(500)).map(|n| id(VARIANTS + n)).collect(),
    });
    Profile::new(data).unwrap()
}

/// The median and the 99th percentile of `times`.
fn percentiles(times: &mut [Duration]) -> (Duration, Duration) {
    times.sort();
    let at = |q: usize| times[(times.len() * q / 100).min(times.len() - 1)];
    (at(50), at(99))
}

#[test]
#[ignore = "a harness: run it with --ignored, in release"]
fn a_register_with_a_large_catalog_answers_in_time() {
    let skus: u64 = std::env::var("KEEL_SKUS").ok().and_then(|n| n.parse().ok()).unwrap_or(100_000);
    let started = Instant::now();
    let profile = large(skus);
    let built = started.elapsed();
    let encoded = profile.encode();
    let started = Instant::now();
    let profile = Profile::decode(&encoded).unwrap();
    let decoded = started.elapsed();

    let dir: PathBuf = std::env::temp_dir().join(format!("keel-latency-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let started = Instant::now();
    let mut runtime = Runtime::open(
        dir.join("store.db"),
        StoreKey::new(&mut [0x4b; 32]),
        Identity {
            device: id::<Device>(0xd1),
            signer: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[7; 32]).unwrap(),
        },
        profile,
        ManualClock::new("2026-10-02T13:00:00Z".parse().unwrap()),
        SeededEntropy::new(1),
        SeededEntropy::new(2),
        Locale::EN_US,
    )
    .unwrap();
    let opened = started.elapsed();
    runtime.sign_in(id(0x501)).unwrap();

    let (mut intents, mut payments, mut tickets, mut items) = (vec![], vec![], vec![], vec![]);
    let mut open: Vec<Id<Order>> = Vec::new();
    // A sequence of its own, so every run rings the same items.
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % skus.max(1)
    };
    for _ in 0..500 {
        let started = Instant::now();
        let order = runtime.start_order(Mode::Takeout).unwrap().order;
        intents.push(started.elapsed());
        for _ in 0..2 {
            let variant = id(VARIANTS + next());
            let started = Instant::now();
            runtime.item(variant).unwrap();
            items.push(started.elapsed());
            let started = Instant::now();
            runtime.add_item(order, variant, &[], NonZeroU32::MIN).unwrap();
            intents.push(started.elapsed());
        }
        open.push(order);
        // Each order is paid once ten more are open, so eleven are at a time.
        if open.len() > 10 {
            let paid = open.remove(0);
            let started = Instant::now();
            runtime.ticket(paid).unwrap();
            tickets.push(started.elapsed());
            let started = Instant::now();
            runtime.pay_cash(paid, Money::from_minor(100_000, Currency::USD)).unwrap();
            payments.push(started.elapsed());
        }
    }
    let started = Instant::now();
    let menu = runtime.menu();
    let menu_time = started.elapsed();
    assert!(
        menu.pages.iter().any(|page| page.buttons.len() == usize::try_from(skus.min(500)).unwrap())
    );
    let _ = std::fs::remove_dir_all(&dir);

    let (intent_50, intent_99) = percentiles(&mut intents);
    let (pay_50, pay_99) = percentiles(&mut payments);
    let (ticket_50, ticket_99) = percentiles(&mut tickets);
    let (item_50, item_99) = percentiles(&mut items);
    println!("catalog: {skus} more items, {} bytes encoded", encoded.len());
    println!(
        "built and checked {built:?}; decoded {decoded:?}; runtime opened {opened:?}; menu {menu_time:?}"
    );
    println!(
        "intent: median {intent_50:?}, 99th percentile {intent_99:?} ({} intents)",
        intents.len()
    );
    println!("paying cash: median {pay_50:?}, 99th percentile {pay_99:?}");
    println!("a ticket: median {ticket_50:?}, 99th percentile {ticket_99:?}");
    println!("an item: median {item_50:?}, 99th percentile {item_99:?}");
    // Bounds far beyond a register's budget, which only something gone badly wrong crosses.
    assert!(intent_99 < Duration::from_millis(250), "an intent's 99th percentile, {intent_99:?}");
    assert!(pay_99 < Duration::from_millis(500), "paying's 99th percentile, {pay_99:?}");
    assert!(item_99 < Duration::from_millis(50), "an item's 99th percentile, {item_99:?}");
}
