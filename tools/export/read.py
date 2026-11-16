#!/usr/bin/env python3
"""Reads a Keel export (ADR-0037, decision 3) with nothing of Keel's: what each business day
took, by tender, added up from the export's events alone.

    python3 tools/export/read.py <export.jsonl>

prints, as JSON, `{"location": ..., "days": {"YYYY-MM-DD": {"cash": [payments, minor], ...}}}`:
for each payment, the business day of its first event and the tender it was started with, if
its last state is captured; what it took is the capture's amount, tip and cash rounding. The
kernel's day report says the same, and `keel-cloud`'s export test checks they agree.
"""

import json
import sys

TENDERS = {0: "cash", 1: "card"}


def main(path):
    payments = {}
    location = None
    with open(path, encoding="utf-8") as lines:
        for number, line in enumerate(lines, 1):
            record = json.loads(line)
            kind = record.get("type")
            if kind == "manifest":
                if record.get("format") != "keel.export" or record.get("version") != 1:
                    sys.exit(f"line {number}: not a keel.export version 1")
                location = record["location"]
            elif kind == "event" and record["stream"]["kind"] == "payment":
                payments.setdefault(record["stream"]["id"], []).append(record)
    days = {}
    for events in payments.values():
        # A stream's events in the order every replica folds them: by HLC, then device, then
        # sequence.
        events.sort(key=lambda e: (e["hlc"]["wallMs"], e["hlc"]["logical"], e["device"], e["seq"]))
        tender, captured, state = None, None, None
        for event in events:
            name, payload = event["schema"]["name"], event["payload"]
            if name == "payment.initiated":
                tender = TENDERS.get(payload["tender"])
                state = "initiated"
            elif name == "payment.captured":
                captured, state = payload, "captured"
            elif name in ("payment.failed", "payment.voided"):
                state = "ended"
        if state != "captured" or tender is None:
            continue
        took = sum(part[0] for part in (captured.get("tip"), captured.get("rounding"), captured["amount"]) if part)
        day = days.setdefault(events[0]["businessDate"], {})
        count, total = day.get(tender, (0, 0))
        day[tender] = (count + 1, total + took)
    print(json.dumps({"location": location, "days": days}, sort_keys=True))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
