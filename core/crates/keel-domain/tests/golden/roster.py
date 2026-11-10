#!/usr/bin/env python3
"""The demo café's roster, built independently of Keel's codec with Python's cbor2, from the same
public keys: its encoding's SHA-256 and length, which keel-domain's known answers pin; and the
same roster naming its cloud, in format 2 (ADR-0028).

Run from anywhere:  python3 core/crates/keel-domain/tests/golden/roster.py
"""

import hashlib
import uuid

import cbor2


def ident(n):
    return uuid.UUID(f"0192f0c1-0000-7000-8000-{n:012x}").bytes


ES256, EDDSA = -7, -8

# The public keys of the software signers the test makes from the secrets [1; 32] (Ed25519),
# [2; 32] (P-256, as an uncompressed SEC1 point) and [3; 32] (Ed25519).
BACK_OFFICE = bytes.fromhex("8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c")
FRONT_COUNTER = bytes.fromhex(
    "04550f471003f3df97c3df506ac797f6721fb1a1fb7b8f6f83d224498a65c88e"
    "24136093d7012e509a73715cbd0b00a3cc0ff4b5c01b3ffa196ab1fb327036b8e6"
)
PATIO = bytes.fromhex("ed4928c628d1c2c6eae90338905995612959273a5c63f93636c14614ac8737d1")
# The cloud's, from the secret [4; 32] (Ed25519).
CLOUD = bytes.fromhex("ca93ac1705187071d67b83c7ff0efe8108e8ec4530575d7726879333dbdabe7c")


def device(n, name, algorithm, key, hub=None):
    record = {1: ident(n), 2: name, 3: [algorithm, key]}
    if hub is not None:
        record[4] = hub
    return record


roster = {
    1: 1,
    2: ident(0xC0FE),
    3: [
        device(0xD0, "Back office", EDDSA, BACK_OFFICE, 2),
        device(0xD1, "Front counter", ES256, FRONT_COUNTER, 1),
        device(0xD2, "Patio", EDDSA, PATIO),
    ],
    4: {1: "192.168.1.20", 2: 7443, 3: "hub.keel-cafe.local"},
}

encoded = cbor2.dumps(roster, canonical=True)
print("roster SHA-256 ", hashlib.sha256(encoded).hexdigest())
print("roster length  ", len(encoded))

# Format 2: the cloud is a device, first in order of identifiers, and the roster names where it is.
cloud = dict(roster)
cloud[1] = 2
cloud[3] = [device(0xCC, "Cloud", EDDSA, CLOUD)] + roster[3]
cloud[5] = {1: ident(0xCC), 2: "203.0.113.10", 3: 8443, 4: "cell.keel-cafe.example"}
encoded = cbor2.dumps(cloud, canonical=True)
print("cloud roster SHA-256 ", hashlib.sha256(encoded).hexdigest())
print("cloud roster length  ", len(encoded))
