"""Planted bugs for keel-hub: see run.py.

Its test runs a provisioned location end to end over real TLS: known answers.
"""

KNOWN_ANSWERS = ["location"]

BUGS = [
    (
        "hub: the loop never ticks the runtime",
        "src/lib.rs",
        "    if runtime.tick_due() {\n        match runtime.on_tick() {",
        "    if false {\n        match runtime.on_tick() {",
        "unit",
    ),
    # Not a bug: leaving out the step's own push of what the device wrote changes nothing a
    # test can see, since the runtime's on_frame and on_tick push it first, and a tick comes
    # every heartbeat.
]
