//! The back office's API (ADR-0028, decision 4): JSON over HTTP, in the cell's process, for one
//! operator, whose bearer token the cell's configuration holds, compared in constant time. TLS is
//! ended in front of the cell, as a cloud's load balancer ends it.
//!
//! | Route | |
//! |---|---|
//! | `GET /api/locations` | The locations the cell serves. |
//! | `GET /api/locations/{id}/replication` | Each device of the location: whether it is connected, and how far the durable copy holds its log. |
//! | `GET /api/locations/{id}/catalog` | The catalog and menu ([`CatalogView`]). |
//! | `POST /api/locations/{id}/catalog/check` | Checks a catalog ([`CatalogEdit`]) whole, publishing nothing: its version, or why not. |
//! | `PUT /api/locations/{id}/catalog` | Publishes a catalog, whose `base` names the version it was read at: 409 if another was published since, 503 while the cell catches up with the location. |
//! | `GET /api/locations/{id}/staff` | The roles and the team ([`StaffView`]); no PIN is ever read back. |
//! | `POST /api/locations/{id}/staff/check`, `PUT /api/locations/{id}/staff` | Checks or publishes staff ([`StaffEdit`]), as the catalog's: a new PIN is hashed with a salt the cell draws. |
//! | `GET /api/locations/{id}/reports/sales?from=&to=` | Sales over business days ([`SalesReport`](crate::reports::SalesReport)), today's by default. |
//! | `GET /api/locations/{id}/settings`, `POST …/settings/check`, `PUT …/settings` | The location's settings ([`SettingsView`], [`SettingsEdit`]), read, checked or published as the catalog is (ADR-0037). |
//! | `GET /api/locations/{id}/dashboard?date=` | A business day's dashboard ([`Dashboard`](crate::dashboard::Dashboard)), today's by default. |
//! | `GET /api/locations/{id}/export` | Everything the location wrote, as JSON Lines (`docs/architecture/export.md`). |
//!
//! Anything else is the back office's single-page app, from the directory the cell is given,
//! its `index.html` for any path it doesn't hold.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use keel_domain::publication;
use keel_events::envelope::Location as LocationRef;
use keel_events::hash::sha256;
use keel_types::{BusinessDate, Clock, Id, OsEntropy};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};

use crate::catalog::{self, CatalogEdit, CatalogView};
use crate::settings::{self, SettingsEdit, SettingsView};
use crate::staff::{self, StaffEdit, StaffView};
use keel_runtime::RuntimeError;

use crate::{CellError, CloudRuntime, Location, dashboard, export, reports};

/// The most a request's body may be: a catalog of the largest profile that may be published.
const MAX_BODY: usize = 64 * 1024 * 1024;

/// What the API serves.
pub struct Cell {
    locations: Vec<Location>,
    token: [u8; 32],
    web: Option<PathBuf>,
}

impl Cell {
    /// The cell serving `locations`, for the operator whose bearer token is `token`, and the back
    /// office from `web`, if given.
    pub fn new(locations: Vec<Location>, token: &str, web: Option<PathBuf>) -> Cell {
        Cell { locations, token: sha256(token.as_bytes()), web }
    }

    /// The locations it serves.
    pub fn locations(&self) -> &[Location] {
        &self.locations
    }

    fn location(&self, id: &str) -> Result<usize, ApiError> {
        let id: Id<LocationRef> = Id::parse(id).map_err(|_| ApiError::NotFound)?;
        self.locations.iter().position(|location| location.id() == id).ok_or(ApiError::NotFound)
    }
}

/// Why a request failed, as its answer says.
#[derive(Debug)]
enum ApiError {
    NotFound,
    NoRoute,
    Unauthorized,
    Invalid(String),
    /// A request the JSON extractor refused, with its own status: 413 for one too large, 415
    /// for one that isn't JSON, 422 for one that isn't what it must be.
    Rejected(StatusCode, String),
    Conflict(String),
    /// The location's cloud hasn't settled its log, and writes nothing until it has.
    Unavailable,
    Failed(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "no such location".to_owned()),
            ApiError::NoRoute => (StatusCode::NOT_FOUND, "no such route".to_owned()),
            ApiError::Rejected(status, message) => (status, message),
            ApiError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "the cell is catching up with the location: try again in a few seconds".to_owned(),
            ),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "a bearer token is needed".into()),
            ApiError::Invalid(message) => (StatusCode::UNPROCESSABLE_ENTITY, message),
            ApiError::Conflict(version) => {
                let body = json!({
                    "error": "another catalog was published since this one was read",
                    "version": version,
                });
                return (StatusCode::CONFLICT, Json(body)).into_response();
            }
            ApiError::Failed(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

impl From<CellError> for ApiError {
    fn from(error: CellError) -> ApiError {
        ApiError::Failed(error.to_string())
    }
}

/// A request the JSON extractor refused, answered with its own status, as JSON.
fn rejected(rejection: &JsonRejection) -> ApiError {
    ApiError::Rejected(rejection.status(), rejection.body_text())
}

async fn no_route() -> ApiError {
    ApiError::NoRoute
}

/// Whether `given` is the token, compared in constant time: their hashes are, every byte.
fn same(given: &[u8], token: &[u8; 32]) -> bool {
    let given = sha256(given);
    given.iter().zip(token).fold(0_u8, |differ, (a, b)| differ | (a ^ b)) == 0
}

async fn authorized(
    State(cell): State<Arc<Cell>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    match given {
        Some(given) if same(given.as_bytes(), &cell.token) => next.run(request).await,
        _ => ApiError::Unauthorized.into_response(),
    }
}

/// Runs `job` on location `index`'s thread, off the server's.
async fn on<R: Send + 'static>(
    cell: &Arc<Cell>,
    index: usize,
    job: impl FnOnce(&mut CloudRuntime) -> R + Send + 'static,
) -> Result<R, ApiError> {
    let cell = Arc::clone(cell);
    tokio::task::spawn_blocking(move || {
        cell.locations.get(index).ok_or(ApiError::NotFound)?.with(job).map_err(ApiError::from)
    })
    .await
    .map_err(|error| ApiError::Failed(error.to_string()))?
}

#[derive(Serialize)]
struct LocationJson {
    id: String,
    name: String,
}

async fn locations(State(cell): State<Arc<Cell>>) -> Json<Vec<LocationJson>> {
    Json(
        cell.locations
            .iter()
            .map(|location| LocationJson {
                id: location.id().to_string(),
                name: location.name().to_owned(),
            })
            .collect(),
    )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceJson {
    id: String,
    name: String,
    /// `hub` for a device that may host the hub, `cloud` for the cell, `device` otherwise.
    role: &'static str,
    hub_priority: Option<u8>,
    connected: bool,
    /// How far the durable copy holds the device's log.
    held: u64,
}

async fn replication(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = cell.location(&id)?;
    let location = cell.locations.get(index).ok_or(ApiError::NotFound)?;
    let roster = location.roster().clone();
    let connected = location.connected();
    let cloud = roster.cloud().map(|cloud| cloud.device);
    let devices: Vec<_> = roster.data().devices.iter().map(|device| device.id).collect();
    let held = on(&cell, index, move |runtime| {
        devices
            .iter()
            .map(|device| runtime.store().head(*device).map(|head| head.seq()))
            .collect::<Result<Vec<u64>, _>>()
    })
    .await?
    .map_err(|error| ApiError::Failed(error.to_string()))?;
    let devices: Vec<DeviceJson> = roster
        .data()
        .devices
        .iter()
        .zip(held)
        .map(|(device, held)| DeviceJson {
            id: device.id.to_string(),
            name: device.name.to_string(),
            role: if Some(device.id) == cloud {
                "cloud"
            } else if device.hub.is_some() {
                "hub"
            } else {
                "device"
            },
            hub_priority: device.hub.map(core::num::NonZeroU8::get),
            connected: connected.contains(&device.id) || Some(device.id) == cloud,
            held,
        })
        .collect();
    Ok(Json(json!({ "location": id, "devices": devices })))
}

async fn read_catalog(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
) -> Result<Json<CatalogView>, ApiError> {
    let index = cell.location(&id)?;
    let view =
        on(&cell, index, |runtime| catalog::view(runtime.profile(), runtime.profile_version()))
            .await?;
    Ok(Json(view))
}

async fn check_catalog(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    edit: Result<Json<CatalogEdit>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = cell.location(&id)?;
    let Json(edit) = edit.map_err(|rejection| rejected(&rejection))?;
    let checked = on(&cell, index, move |runtime| {
        catalog::apply(runtime.profile(), &edit).map(|profile| publication::version(&profile))
    })
    .await?;
    let version = checked.map_err(|error| ApiError::Invalid(error.to_string()))?;
    Ok(Json(json!({ "version": catalog::hex(version) })))
}

async fn publish_catalog(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    edit: Result<Json<CatalogEdit>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = cell.location(&id)?;
    let Json(edit) = edit.map_err(|rejection| rejected(&rejection))?;
    let base = edit.base.clone();
    publish(&cell, index, base, move |profile| {
        catalog::apply(profile, &edit).map_err(|error| ApiError::Invalid(error.to_string()))
    })
    .await
}

/// Publishes the profile `make` makes of the location's current one, once `base`, the version it
/// was read at, is checked to be the current one: in one job, so nothing is published between
/// the two.
async fn publish(
    cell: &Arc<Cell>,
    index: usize,
    base: Option<String>,
    make: impl FnOnce(&keel_domain::profile::Profile) -> Result<keel_domain::profile::Profile, ApiError>
    + Send
    + 'static,
) -> Result<Json<serde_json::Value>, ApiError> {
    let published = on(cell, index, move |runtime| {
        let current = catalog::hex(runtime.profile_version());
        // The version it was read at is required: a write that names none would overwrite a
        // newer publication unseen. Found in review.
        let base = base.ok_or_else(|| {
            ApiError::Invalid("base, the version the profile was read at, is required".into())
        })?;
        if !base.eq_ignore_ascii_case(&current) {
            return Err(ApiError::Conflict(current));
        }
        let profile = make(runtime.profile())?;
        runtime.publish_profile(&profile).map_err(|error| match error {
            RuntimeError::NotSettled => ApiError::Unavailable,
            RuntimeError::Publication(_) => ApiError::Invalid(error.to_string()),
            _ => ApiError::Failed(error.to_string()),
        })
    })
    .await??;
    Ok(Json(json!({ "version": catalog::hex(published) })))
}

async fn read_staff(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
) -> Result<Json<StaffView>, ApiError> {
    let index = cell.location(&id)?;
    let view =
        on(&cell, index, |runtime| staff::view(runtime.profile(), runtime.profile_version()))
            .await?;
    Ok(Json(view))
}

async fn check_staff(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    edit: Result<Json<StaffEdit>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = cell.location(&id)?;
    let Json(edit) = edit.map_err(|rejection| rejected(&rejection))?;
    let checked = on(&cell, index, move |runtime| {
        staff::apply(runtime.profile(), &edit, &mut OsEntropy)
            .map(|profile| publication::version(&profile))
    })
    .await?;
    let version = checked.map_err(|error| ApiError::Invalid(error.to_string()))?;
    Ok(Json(json!({ "version": catalog::hex(version) })))
}

async fn publish_staff(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    edit: Result<Json<StaffEdit>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = cell.location(&id)?;
    let Json(edit) = edit.map_err(|rejection| rejected(&rejection))?;
    let base = edit.base.clone();
    publish(&cell, index, base, move |profile| {
        staff::apply(profile, &edit, &mut OsEntropy)
            .map_err(|error| ApiError::Invalid(error.to_string()))
    })
    .await
}

async fn read_settings(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
) -> Result<Json<SettingsView>, ApiError> {
    let index = cell.location(&id)?;
    let view =
        on(&cell, index, |runtime| settings::view(runtime.profile(), runtime.profile_version()))
            .await?;
    Ok(Json(view))
}

async fn check_settings(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    edit: Result<Json<SettingsEdit>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = cell.location(&id)?;
    let Json(edit) = edit.map_err(|rejection| rejected(&rejection))?;
    let checked = on(&cell, index, move |runtime| {
        settings::apply(runtime.profile(), &edit).map(|profile| publication::version(&profile))
    })
    .await?;
    let version = checked.map_err(|error| ApiError::Invalid(error.to_string()))?;
    Ok(Json(json!({ "version": catalog::hex(version) })))
}

async fn publish_settings(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    edit: Result<Json<SettingsEdit>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let index = cell.location(&id)?;
    let Json(edit) = edit.map_err(|rejection| rejected(&rejection))?;
    let base = edit.base.clone();
    publish(&cell, index, base, move |profile| {
        settings::apply(profile, &edit).map_err(|error| ApiError::Invalid(error.to_string()))
    })
    .await
}

#[derive(Deserialize)]
struct Range {
    from: Option<String>,
    to: Option<String>,
}

async fn sales(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    Query(range): Query<Range>,
) -> Result<Json<reports::SalesReport>, ApiError> {
    let index = cell.location(&id)?;
    let date = |text: Option<String>| {
        text.map(|text| {
            text.parse::<BusinessDate>()
                .map_err(|_| ApiError::Invalid(format!("{text:?} isn't a date, YYYY-MM-DD")))
        })
        .transpose()
    };
    let (from, to) = (date(range.from)?, date(range.to)?);
    let report = on(&cell, index, move |runtime| {
        let today = runtime
            .profile()
            .business_day()
            .business_date_of(runtime.clock().now())
            .map_err(|error| ApiError::Failed(error.to_string()))?;
        let from = from.unwrap_or(today);
        reports::sales(runtime, from, to.unwrap_or(from)).map_err(|error| match error {
            reports::ReportError::Range => ApiError::Invalid(error.to_string()),
            _ => ApiError::Failed(error.to_string()),
        })
    })
    .await??;
    Ok(Json(report))
}

#[derive(Deserialize)]
struct Day {
    date: Option<String>,
}

async fn read_dashboard(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    Query(day): Query<Day>,
) -> Result<Json<dashboard::Dashboard>, ApiError> {
    let index = cell.location(&id)?;
    let date = day
        .date
        .map(|text| {
            text.parse::<BusinessDate>()
                .map_err(|_| ApiError::Invalid(format!("{text:?} isn't a date, YYYY-MM-DD")))
        })
        .transpose()?;
    let found = on(&cell, index, move |runtime| {
        let now = runtime.clock().now();
        let today = runtime
            .profile()
            .business_day()
            .business_date_of(now)
            .map_err(|error| ApiError::Failed(error.to_string()))?;
        dashboard::dashboard(runtime, date.unwrap_or(today), now)
            .map_err(|error| ApiError::Failed(error.to_string()))
    })
    .await??;
    Ok(Json(found))
}

/// Removes an export's file when the request that made it ends, however it ends; and says so to
/// the job writing it, which removes it itself if it finishes after.
struct Remove {
    path: PathBuf,
    gone: Arc<core::sync::atomic::AtomicBool>,
}

impl Drop for Remove {
    fn drop(&mut self) {
        self.gone.store(true, core::sync::atomic::Ordering::SeqCst);
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The location's export (ADR-0037, decision 3): written to a file only the cell can read, on
/// the location's thread, then streamed from it, so it never sits whole in memory; the file is
/// gone once the response has it open.
async fn read_export(
    State(cell): State<Arc<Cell>>,
    Path(id): Path<String>,
    request: Request,
) -> Result<Response, ApiError> {
    let index = cell.location(&id)?;
    let location = cell.locations.get(index).ok_or(ApiError::NotFound)?;
    let devices: Vec<Id<keel_events::envelope::Device>> =
        location.roster().data().devices.iter().map(|device| device.id).collect();
    let dir = std::env::temp_dir();
    let nonce = keel_types::Entropy::next_u64(&mut OsEntropy)
        .map_err(|error| ApiError::Failed(error.to_string()))?;
    let path = dir.join(format!("keel-export-{id}-{nonce:016x}.jsonl"));
    // A HEAD builds no export: found in review, each did.
    if request.method() == axum::http::Method::HEAD {
        let mut response = Response::new(axum::body::Body::empty());
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/x-ndjson"));
        return Ok(response);
    }
    // The file goes however this ends, a download cancelled while it is written among them: the
    // job, which runs on, removes it itself once it finds the request gone. Found in review.
    let gone = Arc::new(core::sync::atomic::AtomicBool::new(false));
    let _guard = Remove { path: path.clone(), gone: Arc::clone(&gone) };
    let written = path.clone();
    let made = on(&cell, index, move |runtime| {
        let made = (|| {
            let file = export::create_private(&written)?;
            let mut out = std::io::BufWriter::new(file);
            export::export(runtime, &devices, runtime.clock().now(), &mut out).map(|_| ())
        })();
        if gone.load(core::sync::atomic::Ordering::SeqCst) {
            let _ = std::fs::remove_file(&written);
        }
        made
    })
    .await?;
    made.map_err(|error| ApiError::Failed(error.to_string()))?;
    let served = ServeFile::new(&path).try_call(request).await;
    let mut response =
        served.map_err(|error| ApiError::Failed(error.to_string()))?.map(axum::body::Body::new);
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/x-ndjson"));
    // Everything the location wrote: kept by no cache.
    headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static("no-store"));
    let disposition = format!("attachment; filename=\"keel-{id}.jsonl\"");
    if let Ok(value) = header::HeaderValue::from_str(&disposition) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    Ok(response)
}

/// The API's routes, and the back office's files.
pub fn router(cell: Arc<Cell>) -> Router {
    let api = Router::new()
        .route("/api/locations", get(locations))
        .route("/api/locations/{id}/replication", get(replication))
        .route("/api/locations/{id}/catalog", get(read_catalog).put(publish_catalog))
        .route("/api/locations/{id}/catalog/check", post(check_catalog))
        .route("/api/locations/{id}/staff", get(read_staff).put(publish_staff))
        .route("/api/locations/{id}/staff/check", post(check_staff))
        .route("/api/locations/{id}/settings", get(read_settings).put(publish_settings))
        .route("/api/locations/{id}/settings/check", post(check_settings))
        .route("/api/locations/{id}/reports/sales", get(sales))
        .route("/api/locations/{id}/dashboard", get(read_dashboard))
        .route("/api/locations/{id}/export", get(read_export))
        // Anything else under /api is no route, not the single-page app.
        .route("/api", any(no_route))
        .route("/api/{*rest}", any(no_route))
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .route_layer(middleware::from_fn_with_state(Arc::clone(&cell), authorized));
    let router = match &cell.web {
        Some(web) => {
            let index = web.join("index.html");
            api.fallback_service(ServeDir::new(web).fallback(ServeFile::new(index)))
        }
        None => api,
    };
    router.with_state(cell)
}
