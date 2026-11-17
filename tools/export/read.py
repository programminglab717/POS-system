#!/usr/bin/env python3
"""Reads a Keel export (ADR-0037, decision 3) with nothing of Keel's: what each business day
took, by tender, added up from the export's events alone.

    python3 tools/export/read.py <export.jsonl>

prints, as JSON, `{"location": ..., "days": {"YYYY-MM-DD": {"cash": [payments, minor], ...}}}`:
for each payment captured, the business day and tender of its first initiation; what it took is
its first capture's amount, tip and cash rounding. The
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
        # sequence. As the kernel folds a payment: its first initiation says its tender, day and
        # currency, and its first capture after it stands, as the money moved. Found in review:
        # the last of each was taken, and a void after a capture dropped it.
        events.sort(key=lambda e: (e["hlc"]["wallMs"], e["hlc"]["logical"], e["device"], e["seq"]))
        tender, day, currency, captured = None, None, None, None
        for event in events:
            name, payload = event["schema"]["name"], event["payload"]
            if name == "payment.initiated" and day is None:
                tender = TENDERS.get(payload["tender"], f"tender {payload['tender']}")
                day, currency = event["businessDate"], payload["amount"][1]
            # Before its initiation, nothing applies; in another currency, a capture doesn't;
            # after one, nothing changes it. A failure or void changes none of that: a capture
            # after one still applies, the card charged after all.
            elif name == "payment.captured" and day is not None and captured is None:
                if payload["amount"][1] == currency:
                    captured = payload
        if captured is None or day is None:
            continue
        took = sum(part[0] for part in (captured.get("tip"), captured.get("rounding"), captured["amount"]) if part)
        totals = days.setdefault(day, {})
        count, total = totals.get(tender, (0, 0))
        totals[tender] = (count + 1, total + took)
    print(json.dumps({"location": location, "days": days}, sort_keys=True))


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
