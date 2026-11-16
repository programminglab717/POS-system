//! A location's export (ADR-0037, decisions 3 to 5): everything its devices wrote, in one JSON
//! Lines file a merchant can read without Keel, as `docs/architecture/export.md` describes it: a
//! manifest, the profile, and every event by device and sequence, its payload's fields named
//! as the kernel's decoders name them, with the bytes its device signed. Written a line at a
//! time, in two passes over the store, so it never sits whole in memory; and read back into a
//! fresh store by [`import`].

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use keel_domain::fields::{Named, json_name, names};
use keel_events::cbor::Value;
use keel_events::envelope::{Actor, Device};
use keel_events::event::SignedEvent;
use keel_events::keys::Signer;
use keel_events::verify::DeviceRegistry;
use keel_runtime::{Imported, Runtime, RuntimeError};
use keel_types::{Clock, Entropy, Id, Timestamp};
use serde_json::{Map, Value as Json, json};

use crate::{Location, catalog, settings, staff};

/// The export's format.
pub const FORMAT: &str = "keel.export";
/// Its version.
pub const VERSION: u32 = 1;
/// How many events are read from the store at once.
const PAGE: u32 = 500;

/// Why an export couldn't be made or read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ExportError {
    /// The store couldn't be read or written.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// The file couldn't be written or read.
    #[error("the export couldn't be written or read: {0}")]
    Io(#[from] std::io::Error),
    /// The business day that ended last can't be told, as on a clock out of range.
    #[error("the business day can't be told: {0}")]
    Day(String),
    /// A line isn't what the format says.
    #[error("line {line} of the export: {what}")]
    Format {
        /// Its number, from 1.
        line: u64,
        /// What is wrong with it.
        what: String,
    },
}

impl From<keel_store::StoreError> for ExportError {
    fn from(error: keel_store::StoreError) -> ExportError {
        ExportError::Runtime(RuntimeError::from(error))
    }
}

/// What an export holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exported {
    /// How many events.
    pub events: u64,
}

/// Calls `each` on every event of `devices`' logs, in order, device by device.
fn each_event<S: Signer, E: Entropy, C: Clock, I: Entropy>(
    runtime: &Runtime<S, E, C, I>,
    devices: &[Id<Device>],
    mut each: impl FnMut(&SignedEvent) -> Result<(), ExportError>,
) -> Result<(), ExportError> {
    for device in devices {
        let mut after = 0;
        loop {
            let page = runtime.store().log(*device, after, PAGE)?;
            for event in &page {
                each(event)?;
                after = event.body().origin_seq.get();
            }
            if page.len() < usize::try_from(PAGE).unwrap_or(usize::MAX) {
                break;
            }
        }
    }
    Ok(())
}

/// `value`, as the export writes a value (export.md, "Values").
fn json_of(value: &Value) -> Json {
    match value {
        Value::Unsigned(n) => json!(n),
        Value::Negative(n) => {
            // -1 - n; beyond an i64, as text, which no Keel payload holds.
            i64::try_from(*n).map_or_else(
                |_| json!(format!("-{}", u128::from(*n).saturating_add(1))),
                |n| json!(!n),
            )
        }
        Value::Bytes(bytes) => match <[u8; 16]>::try_from(bytes.as_slice())
            .ok()
            .and_then(|bytes| Id::<()>::from_bytes(bytes).ok())
        {
            Some(id) => json!(id.to_string()),
            None => json!(hex(bytes)),
        },
        Value::Text(text) => json!(text),
        Value::Array(items) => Json::Array(items.iter().map(json_of).collect()),
        Value::Map(map) => Json::Object(
            map.iter()
                .map(|(key, value)| {
                    let key = match key {
                        Value::Unsigned(n) => n.to_string(),
                        Value::Text(text) => text.clone(),
                        other => json_of(other).to_string(),
                    };
                    (key, json_of(value))
                })
                .collect(),
        ),
        Value::Bool(flag) => json!(flag),
        Value::Null => Json::Null,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::with_capacity(bytes.len().saturating_mul(2)), |mut text, byte| {
        use core::fmt::Write as _;
        let _ = write!(text, "{byte:02x}");
        text
    })
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `bytes` in base64, padded (RFC 4648).
pub fn base64(bytes: &[u8]) -> String {
    let mut text =
        String::with_capacity(bytes.len().saturating_mul(4).saturating_div(3).saturating_add(4));
    for chunk in bytes.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|at| chunk.get(at).copied().unwrap_or(0));
        let n = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        for (at, shift) in [18_u32, 12, 6, 0].into_iter().enumerate() {
            if at <= chunk.len() {
                let index = usize::try_from((n >> shift) & 63).unwrap_or(0);
                text.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// The bytes `text`, padded base64, holds; `None` if it isn't base64.
pub fn unbase64(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    let mut bytes = Vec::with_capacity(text.len().saturating_mul(3).saturating_div(4));
    let (mut n, mut bits) = (0_u32, 0_u32);
    for symbol in text.bytes() {
        let value = ALPHABET.iter().position(|known| *known == symbol)?;
        n = n << 6 | u32::try_from(value).ok()?;
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            bytes.push(u8::try_from((n >> bits) & 0xff).ok()?);
        }
    }
    Some(bytes)
}

fn actor(actor: &Actor) -> Json {
    match actor {
        Actor::TeamMember(id) => json!({ "member": id.to_string() }),
        Actor::Customer(id) => json!({ "customer": id.to_string() }),
        Actor::Integration(id) => json!({ "integration": id.to_string() }),
        Actor::Extension(id) => json!({ "extension": id.to_string() }),
        Actor::System(component) => json!({ "system": component.to_string() }),
    }
}

/// The event's line: its envelope's fields by name, its payload's fields by the names its
/// schema's decoder gives them, and its signed bytes.
fn event_line(event: &SignedEvent) -> Json {
    let body = event.body();
    let payload = body.payload.value().ok();
    let named = payload.as_ref().and_then(|payload| names(&body.schema, payload));
    let payload = match (&payload, &named) {
        (Some(Value::Map(map)), Some(named)) => Json::Object(
            map.iter()
                .map(|(key, value)| {
                    let name = key
                        .as_u64()
                        .and_then(|key| named.iter().find(|(known, _)| *known == key))
                        .map_or_else(|| json_of(key).to_string(), |(_, name)| json_name(name));
                    (name, json_of(value))
                })
                .collect::<Map<String, Json>>(),
        ),
        // A schema this kernel doesn't know, or a payload it can't read: as it is, its keys
        // numbered, for a newer kernel's export to name.
        (Some(value), _) => json_of(value),
        (None, _) => Json::Null,
    };
    json!({
        "type": "event",
        "id": body.event_id.to_string(),
        "stream": { "kind": body.stream.kind.as_str(), "id": body.stream.id.to_string() },
        "device": body.origin_device.to_string(),
        "seq": body.origin_seq.get(),
        "hlc": { "wallMs": body.hlc.wall_ms(), "logical": body.hlc.logical() },
        "businessDate": body.business_date.to_string(),
        "actor": actor(&body.actor),
        "approval": body.approval.map(|approval| approval.to_string()),
        "schema": { "name": body.schema.name.as_str(), "version": body.schema.version.get() },
        "payload": payload,
        "signed": base64(&event.to_bytes()),
    })
}

fn line(out: &mut impl Write, value: &Json) -> Result<(), ExportError> {
    serde_json::to_writer(&mut *out, value).map_err(std::io::Error::from)?;
    out.write_all(b"\n")?;
    Ok(())
}

/// Writes the export of `runtime`'s location to `out`, `devices` being its devices, at `now`.
///
/// # Errors
/// [`ExportError`] if the store can't be read or `out` written.
pub fn export<S: Signer, E: Entropy, C: Clock, I: Entropy>(
    runtime: &Runtime<S, E, C, I>,
    devices: &[Id<Device>],
    now: Timestamp,
    out: &mut impl Write,
) -> Result<Exported, ExportError> {
    // The first pass: how many events, and each schema they hold with its fields' names.
    let mut schemas: BTreeMap<(String, u32), Vec<Named>> = BTreeMap::new();
    let mut events = 0_u64;
    each_event(runtime, devices, |event| {
        events = events.saturating_add(1);
        let body = event.body();
        let key = (body.schema.name.as_str().to_owned(), body.schema.version.get());
        let named = body.payload.value().ok().and_then(|payload| names(&body.schema, &payload));
        let known = schemas.entry(key).or_default();
        for field in named.into_iter().flatten() {
            if !known.contains(&field) {
                known.push(field);
            }
        }
        Ok(())
    })?;
    let profile = runtime.profile();
    let version = runtime.profile_version();
    line(
        out,
        &json!({
            "type": "manifest",
            "format": FORMAT,
            "version": VERSION,
            "location": profile.data().location.to_string(),
            "exportedAt": now.as_micros(),
            "events": events,
            "schemas": schemas.iter().map(|((name, version), fields)| json!({
                "name": name,
                "version": version,
                "fields": fields.iter().map(|(key, field)| json!({ "key": key, "name": json_name(field) })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
    )?;
    let profile_line = json!({
        "type": "profile",
        "version": catalog::hex(version),
        "settings": serde_json::to_value(settings::view(profile, version)).map_err(std::io::Error::from)?,
        "catalog": serde_json::to_value(catalog::view(profile, version)).map_err(std::io::Error::from)?,
        "staff": serde_json::to_value(staff::view(profile, version)).map_err(std::io::Error::from)?,
    });
    line(out, &profile_line)?;
    each_event(runtime, devices, |event| line(out, &event_line(event)))?;
    out.flush()?;
    Ok(Exported { events })
}

/// Reads the export `input` into `runtime`'s store, its events verified with `registry`.
///
/// # Errors
/// [`ExportError::Format`] for a line that isn't what the format says, as a manifest of another
/// format; [`ExportError`] if `input` can't be read or the store written.
pub fn import<S: Signer, E: Entropy, C: Clock, I: Entropy>(
    runtime: &mut Runtime<S, E, C, I>,
    registry: &DeviceRegistry,
    input: impl BufRead,
) -> Result<Imported, ExportError> {
    let mut signed: Vec<Vec<u8>> = Vec::new();
    let mut manifest = false;
    for (number, text) in input.lines().enumerate() {
        let number = u64::try_from(number).unwrap_or(u64::MAX).saturating_add(1);
        let bad = |what: &str| ExportError::Format { line: number, what: what.to_owned() };
        let record: Json = serde_json::from_str(&text?).map_err(|error| bad(&error.to_string()))?;
        match record.get("type").and_then(Json::as_str) {
            Some("manifest") => {
                let format = record.get("format").and_then(Json::as_str);
                let version = record.get("version").and_then(Json::as_u64);
                if format != Some(FORMAT) || version != Some(u64::from(VERSION)) {
                    return Err(bad("not a keel.export of version 1"));
                }
                manifest = true;
            }
            Some("event") if manifest => {
                let text = record
                    .get("signed")
                    .and_then(Json::as_str)
                    .ok_or_else(|| bad("an event with no signed bytes"))?;
                signed.push(unbase64(text).ok_or_else(|| bad("signed bytes that aren't base64"))?);
            }
            Some("profile") if manifest => {}
            _ => return Err(bad("not a manifest, a profile or an event, or before the manifest")),
        }
    }
    Ok(runtime.import(registry, signed.iter().map(Vec::as_slice))?)
}

/// A new file at `path` that only this user can read: an export holds everything a location
/// wrote.
///
/// # Errors
/// [`ExportError::Io`] if it can't be made, as when one is there already.
pub fn create_private(path: &Path) -> Result<File, ExportError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

/// How many escrow exports the cell keeps of each location (ADR-0037, decision 5).
pub const ESCROW_KEPT: usize = 7;

/// Writes `location`'s export to `dir`, in a directory of its own, for the business day that
/// ended last, unless one is there (ADR-0037, decision 5); keeps the [`ESCROW_KEPT`] latest.
/// Written whole under another name, then renamed, so the escrow agent never takes half a file.
///
/// # Errors
/// [`ExportError`] if the store can't be read or the file written.
pub fn escrow(location: &Location, dir: &Path) -> Result<Option<PathBuf>, ExportError> {
    let devices: Vec<Id<Device>> =
        location.roster().data().devices.iter().map(|device| device.id).collect();
    let dir = dir.join(location.id().to_string());
    std::fs::create_dir_all(&dir)?;
    let job_dir = dir.clone();
    let written = location
        .with(move |runtime| -> Result<Option<PathBuf>, ExportError> {
            let now = runtime.clock().now();
            let day = |error: keel_types::BusinessDateError| ExportError::Day(error.to_string());
            let today = runtime.profile().business_day().business_date_of(now).map_err(day)?;
            let ended = today.previous().map_err(day)?;
            let target = job_dir.join(format!("{ended}.jsonl"));
            if target.exists() {
                return Ok(None);
            }
            let partial = job_dir.join(format!("{ended}.jsonl.partial"));
            let _ = std::fs::remove_file(&partial);
            let mut out = std::io::BufWriter::new(create_private(&partial)?);
            export(runtime, &devices, now, &mut out)?;
            out.into_inner().map_err(std::io::IntoInnerError::into_error)?.sync_all()?;
            std::fs::rename(&partial, &target)?;
            Ok(Some(target))
        })
        .map_err(|error| ExportError::Io(std::io::Error::other(error.to_string())))??;
    // The latest kept: their names are their days, which sort as they come.
    let mut kept: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "jsonl"))
        .collect();
    kept.sort();
    let surplus = kept.len().saturating_sub(ESCROW_KEPT);
    for old in kept.iter().take(surplus) {
        std::fs::remove_file(old)?;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::{base64, unbase64};

    #[test]
    fn base64_is_rfc_4648s() {
        for (bytes, text) in [
            (&b""[..], ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(bytes), text);
            assert_eq!(unbase64(text).as_deref(), Some(bytes));
        }
        let every: Vec<u8> = (0..=255).collect();
        assert_eq!(unbase64(&base64(&every)), Some(every));
        assert_eq!(unbase64("Zm9v!"), None);
    }
}
