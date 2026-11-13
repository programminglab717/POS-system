"""Planted bugs for keel-cloud: see run.py.

Its tests run a location with its cloud end to end over real TLS: known answers.
"""

KNOWN_ANSWERS = ["api", "ingest", "publishing"]

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
]
