"""Planted bugs for keel-cloud: see run.py.

Its tests run a location with its cloud end to end over real TLS: known answers.
"""

KNOWN_ANSWERS = ["api", "export", "ingest", "publishing"]

BUGS = [
    (
        "cloud: any kit of a location with a cloud is taken for the cloud's",
        "src/lib.rs",
        "    if roster.cloud().is_none_or(|cloud| cloud.device != kit.device) {",
        "    if roster.cloud().is_none() {",
    ),
    (
        "cloud: a location's loop never runs the jobs handed it",
        "src/lib.rs",
        "                Ok(job) => job(runtime),",
        "                Ok(_) => {}",
    ),
    (
        "cloud: a location's loop never replicates",
        "src/lib.rs",
        "        match keel_hub::step_seeing(runtime, net, STEP_WAIT, seen) {",
        "        match Ok::<bool, RuntimeError>(std::thread::sleep(STEP_WAIT) == ()).inspect(|_| drop(seen)) {",
    ),
    # The back office's API.
    (
        "api: any token is the operator's",
        "src/api.rs",
        "    given.iter().zip(token).fold(0_u8, |differ, (a, b)| differ | (a ^ b)) == 0",
        "    given.iter().zip(token).fold(0_u8, |differ, (a, b)| differ | (a ^ b)) <= 255",
    ),
    (
        "api: a token is the operator's if it begins as theirs",
        "src/api.rs",
        "        Some(given) if same(given.as_bytes(), &cell.token) => next.run(request).await,",
        "        Some(given) if !given.is_empty() => next.run(request).await,",
    ),
    (
        "api: a catalog read before another was published overwrites it",
        "src/api.rs",
        "        if !base.eq_ignore_ascii_case(&current) {\n            return Err(ApiError::Conflict(current));",
        "        if base.is_empty() {\n            return Err(ApiError::Conflict(current));",
    ),
    (
        "api: a check publishes",
        "src/api.rs",
        "        catalog::apply(runtime.profile(), &edit).map(|profile| publication::version(&profile))",
        "        catalog::apply(runtime.profile(), &edit).map(|profile| runtime.publish_profile(&profile).unwrap())",
    ),
    (
        "api: the cell's own device is shown as a register",
        "src/api.rs",
        "            role: if Some(device.id) == cloud {",
        "            role: if Some(device.id) == cloud && false {",
    ),
    (
        "catalog: groups keep the order the editor gave them",
        "src/catalog.rs",
        "    groups.sort_by_key(|group| group.id.to_bytes());",
        "",
    ),
    (
        "catalog: an unknown prefix is taken as plain",
        "src/catalog.rs",
        "                            .map(|(prefix, _)| *prefix)",
        "                            .map_or(Some(Prefix::Plain), |(prefix, _)| Some(*prefix))",
    ),
    (
        "reports: orders of every business day are counted",
        "src/reports.rs",
        "        let Some(date) = order.business_date.filter(|date| (from..=to).contains(date)) else {",
        "        let Some(date) = order.business_date else {",
    ),
    (
        "reports: a range that ends before it begins is taken",
        "src/reports.rs",
        "    if to < from {\n        return Err(ReportError::Range);",
        "    if to < from && false {\n        return Err(ReportError::Range);",
    ),
    (
        "reports: an order's total is its lines', before tax",
        "src/reports.rs",
        "        total = total.checked_add(ticket.total.money)?;",
        "        total = total.checked_add(ticket.subtotal.money)?;",
    ),
    (
        "api: a profile that names no base overwrites what it never read",
        "src/api.rs",
        "        let base = base.ok_or_else(|| {\n            ApiError::Invalid(\"base, the version the profile was read at, is required\".into())\n        })?;",
        "        let base = base.unwrap_or_else(|| catalog::hex(runtime.profile_version()));",
    ),
    (
        "api: a base in capitals is another version",
        "src/api.rs",
        "        if !base.eq_ignore_ascii_case(&current) {",
        "        if *base != current {",
    ),
    (
        "api: a cell that hasn't settled answers that it failed",
        "src/api.rs",
        "            RuntimeError::NotSettled => ApiError::Unavailable,",
        "            RuntimeError::NotSettled => ApiError::Failed(error.to_string()),",
    ),
    (
        "api: what the JSON extractor refuses is always 422",
        "src/api.rs",
        "    ApiError::Rejected(rejection.status(), rejection.body_text())",
        "    ApiError::Rejected(StatusCode::UNPROCESSABLE_ENTITY, rejection.body_text())",
    ),
    (
        "api: an unknown route under /api is the back office's page",
        "src/api.rs",
        "        .route(\"/api/{*rest}\", any(no_route))",
        "",
    ),
    (
        "reports: an item's unit is not its own",
        "src/reports.rs",
        "                .entry((line.name.to_string(), line.quantity.unit().code()))",
        "                .entry((line.name.to_string(), \"ea\"))",
    ),
    (
        "staff: a member who gives no PIN loses theirs",
        "src/staff.rs",
        "            None => profile.member(member_id).and_then(|kept| kept.pin),",
        "            None => None,",
    ),
    (
        "staff: every PIN is hashed with one salt",
        "src/staff.rs",
        "            Some(digits) => Some(PinHash::new(digits, salt(entropy)?).map_err(|error| {",
        "            Some(digits) => Some(PinHash::new(digits, [0; 16]).map_err(|error| {",
    ),
    (
        "staff: a new PIN's digits are in a log line",
        "src/staff.rs",
        "            .field(\"pin\", &self.pin.as_ref().map(|_| \"..\"))",
        "            .field(\"pin\", &self.pin)",
    ),
    (
        "catalog: a deleted item stays in the kitchen's routes",
        "src/catalog.rs",
        "            route.items.retain(|item| items.contains(item));",
        "            route.items.retain(|_| true);",
    ),
    (
        "catalog: a route naming nothing stays",
        "src/catalog.rs",
        "        kitchen.routes.retain(|route| !route.items.is_empty() || !route.modifiers.is_empty());",
        "",
    ),
    # The back office (ADR-0037).
    (
        "settings: a cutoff past 23 reads as a time zone",
        "src/settings.rs",
        "        BusinessDateError::InvalidCutoff { .. } => bad(),",
        "        BusinessDateError::InvalidCutoff { .. } => SettingsError::Field(\"a time zone\".to_owned()),",
    ),
    (
        "settings: a tip prompt is always the screen",
        "src/settings.rs",
        "                    \"terminal\" => TipPrompt::Terminal,",
        "                    \"terminal\" => TipPrompt::Screen,",
    ),
    (
        "dashboard: the week before is the day itself",
        "src/dashboard.rs",
        "    for _ in 0..7 {",
        "    for _ in 0..0 {",
    ),
    (
        "dashboard: open orders aren't counted",
        "src/dashboard.rs",
        "        open_orders: u64::try_from(open.len()).unwrap_or(u64::MAX),",
        "        open_orders: 0,",
    ),
    (
        "dashboard: refunds aren't summed",
        "src/dashboard.rs",
        "        refunded: sum(&mut report.refunds.iter().map(|line| line.amount.money))?,",
        "        refunded: 0,",
    ),
    (
        "export: an identifier is written as hex",
        "src/export.rs",
        "            Some(id) => json!(id.to_string()),",
        "            Some(_) => json!(hex(bytes)),",
    ),
    (
        "export: base64 drops a chunk's last symbol",
        "src/export.rs",
        "            if at <= chunk.len() {",
        "            if at < chunk.len() {",
    ),
    (
        "export: the manifest counts no events",
        "src/export.rs",
        "        events = events.saturating_add(1);",
        "        events = events.saturating_add(0);",
    ),
    (
        "export: a payload's fields are numbered, not named",
        "src/export.rs",
        "                        .map_or_else(|| json_of(key).to_string(), |(_, name)| json_name(name));",
        "                        .map_or_else(|| json_of(key).to_string(), |_| json_of(key).to_string());",
    ),
    (
        "import: any manifest is taken",
        "src/export.rs",
        "                if format != Some(FORMAT) || version != Some(u64::from(VERSION)) {",
        "                if format.is_none() && version.is_none() {",
    ),
    (
        "escrow: every export is kept",
        "src/export.rs",
        "    let surplus = kept.len().saturating_sub(ESCROW_KEPT);",
        "    let surplus = 0_usize;",
    ),
    # Review fixes (ADR-0037).
    (
        "export: a profile's publication is exported, PIN hashes and all",
        "src/export.rs",
        "    event.body().stream.kind.as_str() == keel_domain::publication::STREAM",
        "    event.body().stream.kind.as_str() == \"nothing\"",
    ),
    (
        "import: an export cut short is taken as whole",
        "src/export.rs",
        "        Some(expected) if expected != read => {",
        "        Some(expected) if expected != read && false => {",
    ),
    (
        "import: no manifest is an empty export",
        "src/export.rs",
        "        None => Err(bad(\"no manifest: not an export\".to_owned())),",
        "        None => Ok(imported),",
    ),
    (
        "escrow: a partial file stays",
        "src/export.rs",
        "        let _ = std::fs::remove_file(partial);",
        "        let _ = partial;",
    ),
    (
        "export: a download may be cached",
        "src/api.rs",
        "    headers.insert(header::CACHE_CONTROL, header::HeaderValue::from_static(\"no-store\"));",
        "",
    ),
    (
        "dashboard: hours start at midnight",
        "src/dashboard.rs",
        "    hours.sort_by_key(|sales| (sales.hour < cutoff, sales.hour));",
        "    hours.sort_by_key(|sales| sales.hour);",
    ),
    (
        "dashboard: an even median is the upper middle",
        "src/dashboard.rs",
        "    if sorted.len() % 2 == 1 {",
        "    if sorted.len() % 2 != 2 {",
    ),
    (
        "escrow: the day is written again each time",
        "src/export.rs",
        "                return Ok(None);\n            }\n            let partial",
        "            }\n            let partial",
    ),
    (
        "export: the file served is left behind",
        "src/api.rs",
        "        let _ = std::fs::remove_file(&self.path);",
        "",
    ),
    (
        "languages: a station's language isn't written",
        "src/settings.rs",
        "                    language: station.language.as_deref().map(language).transpose()?,",
        "                    language: None,",
    ),
]
