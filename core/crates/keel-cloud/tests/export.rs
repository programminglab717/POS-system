//! The export both ways (ADR-0037, decision 4): a register's day exported, then imported into a
//! fresh store for another of the location's devices, gives the same events, the same day
//! report and the same stock; and a reader that knows nothing of Keel, `tools/export/read.py`,
//! adds up from the export what the kernel's report says each tender took.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::{NonZeroU8, NonZeroU16, NonZeroU32};
use std::io::Cursor;
use std::path::PathBuf;

use keel_cloud::export::{FORMAT, export, import};
use keel_domain::codec::ReasonCode;
use keel_domain::order::{Mode, Reason};
use keel_domain::payment::Tender;
use keel_domain::profile::{Profile, demo};
use keel_domain::stock::Movement;
use keel_events::envelope::Device;
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_provision::{CloudSpec, DeviceSpec, Kit, Spec, provision};
use keel_runtime::{Identity, RefundTo, ReportKind, Runtime};
use keel_store::StoreKey;
use keel_types::{Clock, Id, Locale, OsEntropy, Quantity, SystemClock, Unit};
use serde_json::Value;

type DeviceRuntime = Runtime<SoftwareSigner, OsEntropy, SystemClock, OsEntropy>;

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("keel-export-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

fn open(kit: &Kit, dir: &Dir) -> DeviceRuntime {
    let mut store_key = kit.store_key;
    Runtime::open(
        dir.0.join("store.db"),
        StoreKey::new(&mut store_key),
        Identity {
            device: kit.device,
            signer: SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &kit.signing_key)
                .unwrap(),
        },
        Profile::decode(&kit.profile).unwrap(),
        SystemClock,
        OsEntropy,
        OsEntropy,
        Locale::EN_US,
    )
    .unwrap()
}

fn spec() -> Spec {
    Spec {
        profile: demo().unwrap().encode(),
        hub_host: "127.0.0.1".into(),
        hub_port: NonZeroU16::new(7450).unwrap(),
        hub_name: "hub.keel-cafe.local".into(),
        cloud: Some(CloudSpec {
            host: "127.0.0.1".into(),
            port: NonZeroU16::new(7451).unwrap(),
            name: "cell.keel-cafe.example".into(),
        }),
        devices: vec![
            DeviceSpec { name: "Back office".into(), hub: NonZeroU8::new(1) },
            DeviceSpec { name: "Front counter".into(), hub: None },
        ],
    }
}

fn reason(code: &str) -> Reason {
    Reason { code: ReasonCode::new(code).unwrap(), note: None }
}

/// A day on the register: three sales in cash, one of them partly returned and kept out of
/// stock; espresso received and counted.
fn a_day(register: &mut DeviceRuntime) {
    let profile = demo().unwrap();
    register.sign_in(profile.data().team[0].id, "1234").unwrap();
    let espresso = profile.data().menu.pages[0].buttons[0];
    let each = |n| Quantity::from_whole(n, Unit::Each).unwrap();
    register.move_stock(espresso, Movement::Received, each(20), None).unwrap();
    let mut first = None;
    for n in 1..=3 {
        let order = register.start_order(Mode::Takeout).unwrap().order;
        register.add_item(order, espresso, &[], NonZeroU32::new(n).unwrap()).unwrap();
        let due = register.ticket(order).unwrap().cash_due.money;
        register.pay_cash(order, due).unwrap();
        first.get_or_insert(order);
    }
    let order = first.unwrap();
    let line = register.ticket(order).unwrap().lines[0].line;
    register
        .return_lines_with(order, &[(line, each(1))], reason("damaged"), RefundTo::Cash, false)
        .unwrap();
    register.count_stock(espresso, each(13), None, None).unwrap();
}

#[test]
fn an_export_imported_is_the_same_location_and_an_independent_reader_agrees() {
    let provisioned = provision(&spec()).unwrap();
    let kit = |name: &str| provisioned.kits.iter().find(|kit| kit.name == name).unwrap().clone();
    let (register_dir, fresh_dir) = (dir("register"), dir("fresh"));
    let mut register = open(&kit("Front counter"), &register_dir);
    a_day(&mut register);
    let devices: Vec<Id<Device>> =
        provisioned.roster.data().devices.iter().map(|device| device.id).collect();
    let mut file = Vec::new();
    let exported = export(&register, &devices, register.clock().now(), &mut file).unwrap();
    assert!(exported.events > 10, "{exported:?}");

    // Every line is JSON: a manifest, the profile, then the events, their payloads named.
    let text = String::from_utf8(file.clone()).unwrap();
    let lines: Vec<Value> = text.lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(
        (lines[0]["type"].as_str(), lines[0]["format"].as_str()),
        (Some("manifest"), Some(FORMAT))
    );
    assert_eq!(lines[0]["events"], exported.events);
    assert_eq!(lines[1]["type"], "profile");
    assert!(!lines[1].to_string().contains("salt"), "no PIN, nor anything of one");
    assert_eq!(u64::try_from(lines.len()).unwrap(), exported.events + 2);
    let moved = lines.iter().find(|line| line["schema"]["name"] == "stock.moved").unwrap();
    assert_eq!(moved["payload"]["quantity"], serde_json::json!([20_000_000, "each"]));
    let counted = lines.iter().find(|line| line["schema"]["name"] == "stock.counted").unwrap();
    assert!(counted["payload"]["asOf"].is_number(), "{counted}");
    let initiated =
        lines.iter().find(|line| line["schema"]["name"] == "payment.initiated").unwrap();
    let order = initiated["payload"]["order"].as_str().unwrap();
    assert!(
        lines.iter().any(|line| line["stream"]["id"] == order),
        "a payment's order is its stream's identifier"
    );

    // Imported into a fresh store of the hub's: the same events, report and stock.
    let mut fresh = open(&kit("Back office"), &fresh_dir);
    let imported = import(&mut fresh, &provisioned.roster.registry(), Cursor::new(&file)).unwrap();
    assert_eq!((imported.stored, imported.quarantined, imported.gaps), (exported.events, 0, 0));
    let register_device = kit("Front counter").device;
    assert_eq!(
        fresh.store().log(register_device, 0, 10_000).unwrap(),
        register.store().log(register_device, 0, 10_000).unwrap()
    );
    let today = register.today().unwrap();
    let (here, there) = (
        register.day_report(today, ReportKind::X).unwrap(),
        fresh.day_report(today, ReportKind::X).unwrap(),
    );
    assert_eq!(
        (here.orders, here.total.money, &here.tenders, &here.refunds),
        (there.orders, there.total.money, &there.tenders, &there.refunds)
    );
    assert_eq!((&here.wasted, &here.counts), (&there.wasted, &there.counts));
    assert_eq!(register.stock().unwrap(), fresh.stock().unwrap());
    // Imported again, nothing changes.
    let again = import(&mut fresh, &provisioned.roster.registry(), Cursor::new(&file)).unwrap();
    assert_eq!((again.stored, again.duplicates), (0, exported.events));

    // The independent reader adds up what each tender took, as the report does.
    let path = register_dir.0.join("export.jsonl");
    std::fs::write(&path, &file).unwrap();
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tools/export/read.py");
    let output = std::process::Command::new("python3")
        .arg("-I")
        .arg(script)
        .arg(&path)
        .output()
        .expect("python3, as CI has");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let read: Value = serde_json::from_slice(&output.stdout).unwrap();
    let day = &read["days"][today.to_string()];
    for tender in &here.tenders {
        let name = match tender.tender {
            Tender::Cash => "cash",
            Tender::Card => "card",
        };
        assert_eq!(
            day[name],
            serde_json::json!([tender.payments, tender.amount.money.minor()]),
            "{read}"
        );
    }
    assert_eq!(day.as_object().unwrap().len(), here.tenders.len(), "{read}");
}

#[test]
fn what_isnt_an_export_is_refused_with_its_line() {
    let provisioned = provision(&spec()).unwrap();
    let kit = provisioned.kits.iter().find(|kit| kit.name == "Back office").unwrap().clone();
    let fresh_dir = dir("refused");
    let mut fresh = open(&kit, &fresh_dir);
    let registry = provisioned.roster.registry();
    let mut refused = |text: &str| {
        import(&mut fresh, &registry, Cursor::new(text.as_bytes().to_vec()))
            .unwrap_err()
            .to_string()
    };
    assert!(refused("{\"type\":\"event\"}\n").contains("line 1"));
    assert!(
        refused("{\"type\":\"manifest\",\"format\":\"other\",\"version\":1}\n")
            .contains("keel.export")
    );
    assert!(
        refused("{\"type\":\"manifest\",\"format\":\"keel.export\",\"version\":1,\"events\":0}\nnot json\n")
            .contains("line 2")
    );
    // No manifest, or cut short, is refused: found in review, each imported as if whole.
    assert!(refused("").contains("no manifest"));
    let short = "{\"type\":\"manifest\",\"format\":\"keel.export\",\"version\":1,\"events\":2}\n{\"type\":\"event\",\"signed\":\"AAAA\"}\n";
    assert!(refused(short).contains("cut short"));
    let forged = "{\"type\":\"manifest\",\"format\":\"keel.export\",\"version\":1,\"events\":1}\n{\"type\":\"event\",\"signed\":\"AAAA\"}\n";
    let imported = import(&mut fresh, &registry, Cursor::new(forged.as_bytes().to_vec())).unwrap();
    assert_eq!(
        (imported.stored, imported.quarantined),
        (0, 1),
        "bytes no device signed are quarantined"
    );
}

/// Escrow (ADR-0037, decision 5): the export of the day that ended last, written once, read
/// back as an export, and the latest seven kept.
#[test]
fn escrow_writes_the_day_once_and_keeps_seven() {
    let provisioned = provision(&spec()).unwrap();
    let cell_dir = dir("escrow-cell");
    let kit = cell_dir.0.join("kit");
    provisioned.cloud.as_ref().unwrap().write(&kit).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let location = keel_cloud::Location::open(&kit, listener).unwrap();
    let escrow_dir = dir("escrow");
    let held = escrow_dir.0.join(location.id().to_string());
    std::fs::create_dir_all(&held).unwrap();
    for day in 1..=8 {
        std::fs::write(held.join(format!("2026-01-{day:02}.jsonl")), "old").unwrap();
    }
    let written =
        keel_cloud::export::escrow(&location, &escrow_dir.0).unwrap().expect("the day that ended");
    let first = std::fs::read_to_string(&written).unwrap();
    let manifest: Value = serde_json::from_str(first.lines().next().unwrap()).unwrap();
    assert_eq!(manifest["format"], FORMAT);
    assert_eq!(
        keel_cloud::export::escrow(&location, &escrow_dir.0).unwrap(),
        None,
        "written once a day"
    );
    // Taken away by the escrow agent, it isn't written again: found in review, it was, every
    // ten minutes. And a partial file left by a failed day goes.
    std::fs::rename(&written, escrow_dir.0.join("taken.jsonl")).unwrap();
    std::fs::write(held.join("2026-01-09.jsonl.partial"), "half").unwrap();
    assert_eq!(
        keel_cloud::export::escrow(&location, &escrow_dir.0).unwrap(),
        None,
        "the agent took it"
    );
    assert!(!held.join("2026-01-09.jsonl.partial").exists());
    std::fs::rename(escrow_dir.0.join("taken.jsonl"), &written).unwrap();
    let mut kept: Vec<String> = std::fs::read_dir(&held)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| {
            std::path::Path::new(name).extension().is_some_and(|extension| extension == "jsonl")
        })
        .collect();
    kept.sort();
    assert_eq!(kept.len(), keel_cloud::export::ESCROW_KEPT, "{kept:?}");
    assert_eq!(kept.last().unwrap(), &written.file_name().unwrap().to_string_lossy().into_owned());
    assert!(!kept.contains(&"2026-01-01.jsonl".to_owned()), "the oldest go");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&written).unwrap().permissions().mode() & 0o777, 0o600);
    }
}

/// The dashboard of a day rung (ADR-0037, decision 2): the day's report's figures, the hour
/// its orders closed in, its top item; and a week on, the day as its week before.
#[test]
fn the_dashboard_has_the_day_and_a_week_on_has_it_as_the_week_before() {
    let provisioned = provision(&spec()).unwrap();
    let register_dir = dir("dashboard");
    let kit = provisioned.kits.iter().find(|kit| kit.name == "Front counter").unwrap().clone();
    let mut register = open(&kit, &register_dir);
    a_day(&mut register);
    let today = register.today().unwrap();
    let now = register.clock().now();
    let report = register.day_report(today, ReportKind::X).unwrap();
    let dashboard = keel_cloud::dashboard::dashboard(&register, today, now).unwrap();
    assert_eq!((dashboard.day.orders, dashboard.day.total), (3, report.total.money.minor()));
    assert_eq!(
        dashboard.day.refunded,
        report.refunds.iter().map(|line| line.amount.money.minor()).sum::<i64>()
    );
    assert!(dashboard.day.refunded > 0, "the return refunded");
    assert_eq!(dashboard.week_before.orders, 0);
    assert_eq!(dashboard.hours.iter().map(|hour| hour.orders).sum::<u64>(), 3);
    assert_eq!(dashboard.top_items[0].name, "Espresso");
    assert_eq!((dashboard.open_orders, dashboard.oldest_open_seconds), (0, None));
    assert!(dashboard.below_zero.is_empty());
    // Open an order: it shows, with its age.
    let open = register.start_order(Mode::Takeout).unwrap().order;
    let espresso = demo().unwrap().data().menu.pages[0].buttons[0];
    register.add_item(open, espresso, &[], NonZeroU32::MIN).unwrap();
    let now = register.clock().now();
    let dashboard = keel_cloud::dashboard::dashboard(&register, today, now).unwrap();
    assert_eq!(dashboard.open_orders, 1);
    assert!(dashboard.oldest_open_seconds.is_some());
    let mut week_on = today;
    for _ in 0..7 {
        week_on = week_on.next().unwrap();
    }
    let later = keel_cloud::dashboard::dashboard(&register, week_on, now).unwrap();
    assert_eq!((later.day.orders, later.week_before.orders), (0, 3));
    assert_eq!(later.week_before.total, report.total.money.minor());
}

/// The independent reader folds a payment as the kernel does (ADR-0037, decision 4): the first
/// capture after the first initiation stands, whatever follows; a capture after a failure still
/// applies; nothing before an initiation does. Found in review: it took the last of each.
#[test]
fn the_reader_folds_payments_as_the_kernel_does() {
    let event = |payment: &str, seq: u64, schema: &str, payload: Value| {
        serde_json::json!({
            "type": "event", "stream": { "kind": "payment", "id": payment }, "device": "d1",
            "seq": seq, "hlc": { "wallMs": seq, "logical": 0 }, "businessDate": "2026-11-13",
            "schema": { "name": schema, "version": 1 }, "payload": payload,
        })
    };
    let initiated = |tender: u64| serde_json::json!({ "tender": tender, "amount": [500, "USD"] });
    let captured = |minor: i64| serde_json::json!({ "amount": [minor, "USD"] });
    let reason = serde_json::json!({ "reason": "declined" });
    let lines = [
        serde_json::json!({ "type": "manifest", "format": "keel.export", "version": 1, "location": "l", "events": 0 }),
        // Captured, then voided and captured again: the first capture stands.
        event("p1", 1, "payment.initiated", initiated(0)),
        event("p1", 2, "payment.captured", captured(500)),
        event("p1", 3, "payment.voided", reason.clone()),
        event("p1", 4, "payment.captured", captured(900)),
        event("p1", 5, "payment.initiated", initiated(1)),
        // Failed, then captured: charged after all.
        event("p2", 6, "payment.initiated", initiated(1)),
        event("p2", 7, "payment.failed", reason),
        event("p2", 8, "payment.captured", captured(300)),
        // A capture before any initiation is nothing.
        event("p3", 9, "payment.captured", captured(700)),
        event("p3", 10, "payment.initiated", initiated(0)),
    ];
    let text: String = lines.iter().map(|line| line.to_string() + "\n").collect();
    let dir = dir("reader");
    let path = dir.0.join("export.jsonl");
    std::fs::write(&path, text).unwrap();
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../tools/export/read.py");
    let output =
        std::process::Command::new("python3").arg("-I").arg(script).arg(&path).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let read: Value = serde_json::from_slice(&output.stdout).unwrap();
    let day = &read["days"]["2026-11-13"];
    assert_eq!(*day, serde_json::json!({ "cash": [1, 500], "card": [1, 300] }), "{read}");
}
