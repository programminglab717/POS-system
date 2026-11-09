"""Planted bugs for keel-provision: see run.py. keel-hub's end-to-end test exercises it."""

ALSO = ["keel-hub"]

BUGS = [
    (
        "provision: the hub's certificate lacks the roster's hub name",
        "src/lib.rs",
        "        if device.hub.is_some() {\n            names.push(spec.hub_name.clone());",
        "        if device.hub.is_none() {\n            names.push(spec.hub_name.clone());",
        "unit",
    ),
    (
        "provision: devices are listed as they were asked for",
        "src/lib.rs",
        "    devices.sort_by_key(|device| device.id.to_bytes());",
        "    devices.reverse();",
        "unit",
    ),
    (
        "provision: a kit's store key is its signing key",
        "src/lib.rs",
        "            store_key: random(&mut entropy)?,",
        "            store_key: signing_key,",
        "unit",
    ),
]
