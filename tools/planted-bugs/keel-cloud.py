"""Planted bugs for keel-cloud: see run.py.

Its tests run a location with its cloud end to end over real TLS: known answers.
"""

KNOWN_ANSWERS = ["ingest", "publishing"]

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
]
