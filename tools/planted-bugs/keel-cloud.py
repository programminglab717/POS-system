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
        "        if edit.base.as_ref().is_some_and(|base| *base != current) {",
        "        if edit.base.as_ref().is_some_and(|base| base.is_empty()) {",
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
]
