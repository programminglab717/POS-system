"""Planted bugs for keel-hub: see run.py.

Its test runs a provisioned location end to end over real TLS: known answers.
"""

KNOWN_ANSWERS = ["location"]

BUGS = [
    (
        "hub: the loop never ticks the runtime",
        "src/lib.rs",
        "    if runtime.next_tick().is_some_and(|at| at <= runtime.clock().now()) {",
        "    if runtime.next_tick().is_some_and(|at| at < at) {",
        "unit",
    ),
    # Not a bug: leaving out the step's own push of what the device wrote changes nothing a
    # test can see, since the runtime's on_frame and on_tick push it first, and a tick comes
    # every heartbeat.
]
