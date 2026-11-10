//! The back office's API (ADR-0028, decision 4), through its router in the process: the
//! operator's token, the catalog read, checked, published and refused when stale, each device's
//! replication, and a sales report whose totals are the register's own tickets', for sales rung
//! on a register over real TLS on the host.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic,
    reason = "test code"
)]

use core::num::{NonZeroU8, NonZeroU16, NonZeroU32};
use core::time::Duration;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt as _;
use keel_cloud::Location;
use keel_cloud::api::{Cell, router};
use keel_domain::order::Mode;
use keel_domain::profile::{Profile, demo};
use keel_events::keys::{SignatureAlgorithm, SoftwareSigner};
use keel_net::Net;
use keel_provision::{CloudSpec, DeviceSpec, Kit, Provisioned, Spec, provision};
use keel_runtime::{Identity, Runtime};
use keel_store::StoreKey;
use keel_types::{Currency, Locale, Money, OsEntropy, SystemClock};
use serde_json::{Value, json};
use tower::ServiceExt as _;

type DeviceRuntime = Runtime<SoftwareSigner, OsEntropy, SystemClock, OsEntropy>;

const TOKEN: &str = "test-operator-token-0123456789abcdef";

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(name: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("keel-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    Dir(path)
}

fn bound() -> (TcpListener, u16) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn location(hub_port: u16, cloud_port: u16) -> Provisioned {
    provision(&Spec {
        profile: demo().unwrap().encode(),
        hub_host: "127.0.0.1".into(),
        hub_port: NonZeroU16::new(hub_port).unwrap(),
        hub_name: "hub.keel-cafe.local".into(),
        cloud: Some(CloudSpec {
            host: "127.0.0.1".into(),
            port: NonZeroU16::new(cloud_port).unwrap(),
            name: "cell.keel-cafe.example".into(),
        }),
        devices: vec![
            DeviceSpec { name: "Back office".into(), hub: NonZeroU8::new(1) },
            DeviceSpec { name: "Front counter".into(), hub: None },
        ],
    })
    .unwrap()
}

/// The cell serving the location, its store in `dir`.
fn cell(provisioned: &Provisioned, listener: TcpListener, dir: &Dir) -> Router {
    let kit = dir.0.join("cloud");
    provisioned.cloud.as_ref().unwrap().write(&kit).unwrap();
    let location = Location::open(&kit, listener).unwrap();
    router(Arc::new(Cell::new(vec![location], TOKEN, None)))
}

/// `method` on `path`, with the operator's token unless `token` says otherwise, and `body`:
/// the answer's status and its JSON.
async fn call(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let request = match body {
        Some(body) => request
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap(),
        None => request.body(Body::empty()).unwrap(),
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
    (status, json)
}

async fn get(app: &Router, path: &str) -> (StatusCode, Value) {
    call(app, Method::GET, path, Some(TOKEN), None).await
}

#[tokio::test(flavor = "current_thread")]
async fn only_the_operator_s_token_is_taken() {
    let (listener, port) = bound();
    let provisioned = location(7443, port);
    let dir = dir("token");
    let app = cell(&provisioned, listener, &dir);
    assert_eq!(
        call(&app, Method::GET, "/api/locations", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let wrong = call(&app, Method::GET, "/api/locations", Some("not-it"), None).await;
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    // One that only begins as the token is no more the token.
    let prefix = &TOKEN[..TOKEN.len() - 1];
    assert_eq!(
        call(&app, Method::GET, "/api/locations", Some(prefix), None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, locations) = get(&app, "/api/locations").await;
    assert_eq!(status, StatusCode::OK);
    let id = provisioned.roster.data().location.to_string();
    assert_eq!(locations, json!([{ "id": id, "name": demo().unwrap().data().name.to_string() }]));
    // A location the cell doesn't serve, or that isn't an identifier.
    assert_eq!(get(&app, "/api/locations/nope/catalog").await.0, StatusCode::NOT_FOUND);
    let other = "0192f0c1-0000-7000-8000-000000000bad";
    assert_eq!(
        get(&app, &format!("/api/locations/{other}/catalog")).await.0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_catalog_is_read_checked_and_published_and_refused_when_stale() {
    let (listener, port) = bound();
    let provisioned = location(7443, port);
    let dir = dir("catalog");
    let app = cell(&provisioned, listener, &dir);
    let base = format!("/api/locations/{}/catalog", provisioned.roster.data().location);
    let (status, read) = get(&app, &base).await;
    assert_eq!(status, StatusCode::OK);
    let provisioned_version = keel_domain::publication::version(&demo().unwrap()).to_hex();
    assert_eq!(read["version"], json!(provisioned_version));
    assert_eq!((read["currency"].clone(), read["minorUnits"].clone()), (json!("USD"), json!(2)));
    let espresso = read["items"]
        .as_array()
        .unwrap()
        .iter()
        .position(|item| item["name"] == "Espresso")
        .unwrap();
    assert_eq!(read["items"][espresso]["variants"][0]["price"], json!(325));
    assert!(!read["taxCategories"].as_array().unwrap().is_empty());
    assert_eq!(read["pages"][0]["name"], json!("Coffee"));

    // The catalog as read writes back as it was: its version is the same.
    let edit = |items: &Value| json!({ "base": read["version"], "items": items, "groups": read["groups"], "pages": read["pages"] });
    let (status, checked) =
        call(&app, Method::POST, &format!("{base}/check"), Some(TOKEN), Some(edit(&read["items"])))
            .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(checked["version"], read["version"]);

    // A price changed checks to another version, and publishes as it.
    let mut items = read["items"].clone();
    items[espresso]["variants"][0]["price"] = json!(340);
    let (status, checked) =
        call(&app, Method::POST, &format!("{base}/check"), Some(TOKEN), Some(edit(&items))).await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(checked["version"], read["version"]);
    // Nothing is published by a check.
    assert_eq!(get(&app, &base).await.1["version"], read["version"]);
    // Until the cell has heard its location's hub, or waited to be an island, it writes nothing
    // to the cloud's log. Found in review: it published before it had settled.
    let (status, waiting) = call(&app, Method::PUT, &base, Some(TOKEN), Some(edit(&items))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{waiting}");
    assert_eq!(get(&app, &base).await.1["version"], read["version"]);
    tokio::time::sleep(keel_runtime::ISLAND_AFTER).await;
    // A base in capitals is the same version.
    let mut upper = edit(&items);
    upper["base"] = json!(read["version"].as_str().unwrap().to_uppercase());
    let (status, published) = call(&app, Method::PUT, &base, Some(TOKEN), Some(upper)).await;
    assert_eq!(status, StatusCode::OK, "{published}");
    assert_eq!(published["version"], checked["version"]);
    let (_, again) = get(&app, &base).await;
    assert_eq!(again["version"], published["version"]);
    assert_eq!(again["items"][espresso]["variants"][0]["price"], json!(340));

    // Written against the version read before: another has been published since.
    items[espresso]["variants"][0]["price"] = json!(355);
    let (status, conflict) = call(&app, Method::PUT, &base, Some(TOKEN), Some(edit(&items))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["version"], published["version"]);
    assert_eq!(get(&app, &base).await.1["version"], published["version"]);

    // What the kernel refuses, and what never reaches it.
    let mut broken = again["items"].clone();
    broken[espresso]["variants"][0]["id"] = json!("not-an-id");
    let body = json!({ "items": broken, "groups": again["groups"], "pages": again["pages"] });
    let (status, refused) =
        call(&app, Method::POST, &format!("{base}/check"), Some(TOKEN), Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(refused["error"].as_str().unwrap().contains("isn't an identifier"), "{refused}");
    let mut groups = again["groups"].clone();
    groups[0]["min"] = json!(3);
    groups[0]["max"] = json!(1);
    let body = json!({ "items": again["items"], "groups": groups, "pages": again["pages"] });
    let (status, refused) =
        call(&app, Method::POST, &format!("{base}/check"), Some(TOKEN), Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{refused}");
    // A button for a variant the catalog doesn't have.
    let mut pages = again["pages"].clone();
    pages[0]["buttons"][0] = json!("0192f0c1-0000-7000-8000-00000000dead");
    let body = json!({ "items": again["items"], "groups": again["groups"], "pages": pages });
    let (status, _) = call(&app, Method::PUT, &base, Some(TOKEN), Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    // A write that names no base would overwrite what it never read.
    let body =
        json!({ "items": again["items"], "groups": again["groups"], "pages": again["pages"] });
    let (status, refused) = call(&app, Method::PUT, &base, Some(TOKEN), Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(refused["error"].as_str().unwrap().contains("base"), "{refused}");
    // A body that isn't JSON says so, as JSON.
    let request = Request::builder()
        .method(Method::POST)
        .uri(format!("{base}/check"))
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Content-Type", "text/plain")
        .body(Body::from("{}"))
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert!(serde_json::from_slice::<Value>(&bytes).unwrap()["error"].is_string());
    // A route the API doesn't have is no route, not the back office's page.
    let (status, none) = get(&app, "/api/nowhere").await;
    assert_eq!((status, none["error"].clone()), (StatusCode::NOT_FOUND, json!("no such route")));
    assert_eq!(
        call(&app, Method::GET, "/api/nowhere", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    // An unknown field.
    let body = json!({ "items": [], "groups": [], "pages": [], "extra": 1 });
    let (status, _) =
        call(&app, Method::POST, &format!("{base}/check"), Some(TOKEN), Some(body)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(get(&app, &base).await.1["version"], published["version"]);
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

/// The register's two sales, rung over its network until the cloud holds them: their totals,
/// as its own tickets give them, their business date, and the register.
fn two_sales(
    register_kit: &Kit,
    roster: keel_domain::roster::Roster,
    register_dir: &Dir,
) -> (Vec<i64>, keel_types::BusinessDate, keel_types::Id<keel_events::envelope::Device>) {
    let mut register = open(register_kit, register_dir);
    register.sign_in(demo().unwrap().data().team[0].id).unwrap();
    let first = register.start_sync(roster.clone()).unwrap();
    let net = Net::device(roster, register_kit.credentials()).unwrap();
    for frame in first {
        net.send(frame.to, frame.frame);
    }
    let started = Instant::now();
    let mut tickets = Vec::new();
    while started.elapsed() < Duration::from_secs(60) {
        keel_hub::step(&mut register, &net, Duration::from_millis(50)).unwrap();
        let status = register.sync_status().unwrap().unwrap();
        if tickets.len() < 2 && status.settled && status.hub_reachable {
            let order = register.start_order(Mode::Takeout).unwrap().order;
            let pages = &register.profile().data().menu.pages[0];
            let (first, second) = (pages.buttons[0], pages.buttons[1]);
            register.add_item(order, first, &[], NonZeroU32::MIN).unwrap();
            register.add_item(order, second, &[], NonZeroU32::new(2).unwrap()).unwrap();
            let paid = register.pay_cash(order, Money::from_minor(10_000, Currency::USD)).unwrap();
            tickets.push(paid);
        }
        if tickets.len() == 2 && status.backed_up == status.head {
            break;
        }
    }
    let date = register
        .profile()
        .business_day()
        .business_date_of(keel_types::Clock::now(register.clock()))
        .unwrap();
    let totals: Vec<_> = tickets
        .iter()
        .map(|paid| register.ticket(paid.ticket.order).unwrap().total.money.minor())
        .collect();
    (totals, date, register_kit.device)
}

#[tokio::test(flavor = "current_thread")]
async fn the_report_totals_are_the_register_s_tickets_and_replication_shows_who_is_connected() {
    let (hub_listener, hub_port) = bound();
    let (cloud_listener, cloud_port) = bound();
    let provisioned = location(hub_port, cloud_port);
    let kit = |name: &str| provisioned.kits.iter().find(|kit| kit.name == name).unwrap().clone();
    let (hub_kit, register_kit) = (kit("Back office"), kit("Front counter"));
    let roster = provisioned.roster.clone();
    let (cell_dir, hub_dir, register_dir) =
        (dir("report-cell"), dir("report-hub"), dir("report-reg"));
    let app = cell(&provisioned, cloud_listener, &cell_dir);
    let id = roster.data().location;

    // The hub and the register, rung on its own thread.
    let roster_for_hub = roster.clone();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hub_stop = Arc::clone(&stop);
    let hub = std::thread::spawn(move || {
        let mut hub = open(&hub_kit, &hub_dir);
        let first = hub.start_sync(roster_for_hub.clone()).unwrap();
        let net = Net::hub(hub_listener, roster_for_hub, hub_kit.credentials()).unwrap();
        for frame in first {
            net.send(frame.to, frame.frame);
        }
        while !hub_stop.load(std::sync::atomic::Ordering::Relaxed) {
            keel_hub::step(&mut hub, &net, Duration::from_millis(50)).unwrap();
        }
    });
    let register = std::thread::spawn(move || two_sales(&register_kit, roster, &register_dir));
    let (totals, date, register_device) =
        tokio::task::spawn_blocking(move || register.join().unwrap()).await.unwrap();
    assert_eq!(totals.len(), 2, "the register rang two sales");

    let path = format!("/api/locations/{id}/reports/sales?from={date}&to={date}");
    let (status, report) = get(&app, &path).await;
    assert_eq!(status, StatusCode::OK, "{report}");
    assert_eq!(report["orders"], json!(2));
    assert_eq!(report["total"], json!(totals.iter().sum::<i64>()));
    assert_eq!(
        report["days"],
        json!([{ "date": date.to_string(), "orders": 2, "total": totals.iter().sum::<i64>() }])
    );
    assert_eq!(report["tenders"][0]["tender"], json!("cash"));
    assert_eq!(report["tenders"][0]["payments"], json!(2));
    assert_eq!(report["tenders"][0]["amount"], report["total"]);
    let items = report["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let quantities: Vec<&str> =
        items.iter().map(|item| item["quantity"].as_str().unwrap()).collect();
    assert!(items.iter().all(|item| item["unit"] == "each"), "{items:?}");
    assert_eq!(report["otherCurrency"], json!(0));
    assert!(quantities.contains(&"2") && quantities.contains(&"4"), "{quantities:?}");
    // A range with no sale in it.
    let (status, empty) =
        get(&app, &format!("/api/locations/{id}/reports/sales?from=2000-01-01&to=2000-01-31"))
            .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!((empty["orders"].clone(), empty["total"].clone()), (json!(0), json!(0)));
    assert_eq!(empty["items"], json!([]));
    // Today's by default, and a range that ends before it begins refused.
    let (status, today) = get(&app, &format!("/api/locations/{id}/reports/sales")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(today["from"], json!(date.to_string()));
    let (status, _) =
        get(&app, &format!("/api/locations/{id}/reports/sales?from={date}&to=2000-01-01")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    let (status, _) = get(&app, &format!("/api/locations/{id}/reports/sales?from=yesterday")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    // The hub is connected, the register isn't (it reaches the hub alone), and the durable copy
    // holds every event of the register's.
    let (status, replication) = get(&app, &format!("/api/locations/{id}/replication")).await;
    assert_eq!(status, StatusCode::OK);
    let devices = replication["devices"].as_array().unwrap();
    assert_eq!(devices.len(), 3);
    let by_name = |name: &str| devices.iter().find(|device| device["name"] == name).unwrap();
    assert_eq!(by_name("Back office")["role"], json!("hub"));
    assert_eq!(by_name("Back office")["hubPriority"], json!(1));
    assert_eq!(by_name("Back office")["connected"], json!(true));
    assert_eq!(by_name("Front counter")["role"], json!("device"));
    assert_eq!(by_name("Front counter")["connected"], json!(false));
    assert!(by_name("Front counter")["held"].as_u64().unwrap() > 0);
    assert_eq!(by_name("Cloud")["role"], json!("cloud"));
    assert_eq!(by_name("Front counter")["id"], json!(register_device.to_string()));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    tokio::task::spawn_blocking(move || hub.join().unwrap()).await.unwrap();
}
