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
//! | `PUT /api/locations/{id}/catalog` | Publishes a catalog: 409 if another was published since the one it was read at. |
//! | `GET /api/locations/{id}/reports/sales?from=&to=` | Sales over business days ([`SalesReport`](crate::reports::SalesReport)), today's by default. |
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
use axum::routing::{get, post};
use axum::{Json, Router};
use keel_domain::publication;
use keel_events::envelope::Location as LocationRef;
use keel_events::hash::sha256;
use keel_types::{BusinessDate, Clock, Id};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tower_http::services::{ServeDir, ServeFile};

use crate::catalog::{self, CatalogEdit, CatalogView};
use crate::{CellError, CloudRuntime, Location, reports};

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
    Unauthorized,
    Invalid(String),
    Conflict(String),
    Failed(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "no such location".to_owned()),
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
    let Json(edit) = edit.map_err(|rejection| ApiError::Invalid(rejection.body_text()))?;
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
    let Json(edit) = edit.map_err(|rejection| ApiError::Invalid(rejection.body_text()))?;
    // Checked against the version it was read at, and published, in one job: nothing is
    // published between the two.
    let published = on(&cell, index, move |runtime| {
        let current = catalog::hex(runtime.profile_version());
        if edit.base.as_ref().is_some_and(|base| *base != current) {
            return Err(ApiError::Conflict(current));
        }
        let profile = catalog::apply(runtime.profile(), &edit)
            .map_err(|error| ApiError::Invalid(error.to_string()))?;
        runtime.publish_profile(&profile).map_err(|error| ApiError::Failed(error.to_string()))
    })
    .await??;
    Ok(Json(json!({ "version": catalog::hex(published) })))
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

/// The API's routes, and the back office's files.
pub fn router(cell: Arc<Cell>) -> Router {
    let api = Router::new()
        .route("/api/locations", get(locations))
        .route("/api/locations/{id}/replication", get(replication))
        .route("/api/locations/{id}/catalog", get(read_catalog).put(publish_catalog))
        .route("/api/locations/{id}/catalog/check", post(check_catalog))
        .route("/api/locations/{id}/reports/sales", get(sales))
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
