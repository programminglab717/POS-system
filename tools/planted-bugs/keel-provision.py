"""Planted bugs for keel-provision: see run.py. Its own tests read its certificates and files
back; keel-hub's end-to-end test exercises the rest."""

KNOWN_ANSWERS = ["kit"]

ALSO = ["keel-hub"]

BUGS = [
    (
        "provision: the hub's certificate lacks the roster's hub name",
        "src/lib.rs",
        "        kits.push(kit(id, &name, hub.then_some(spec.hub_name.as_str()), signing_key)?);",
        "        kits.push(kit(id, &name, (!hub).then_some(spec.hub_name.as_str()), signing_key)?);",
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
        "provision: a secret file is readable by anyone",
        "src/lib.rs",
        "        options.mode(0o600);",
        "        options.mode(0o644);",
        "unit",
    ),
    (
        "provision: a secret file there already keeps its mode",
        "src/lib.rs",
        "        if path.exists() {",
        "        if path.exists() && false {",
        "unit",
    ),
    (
        "provision: a kit's directory may be entered by anyone",
        "src/lib.rs",
        "        private_dir(dir)?;",
        "        std::fs::create_dir_all(dir)?;",
        "unit",
    ),
    (
        "provision: the hub's certificate can't serve",
        "src/lib.rs",
        "            usages.push(ExtendedKeyUsagePurpose::ServerAuth);",
        "",
        "unit",
    ),
    (
        "provision: a device's certificate lives as long as the CA's",
        "src/lib.rs",
        "        let mut params = valid(CertificateParams::new(names)?, now, DEVICE_LIFE)?;",
        "        let mut params = valid(CertificateParams::new(names)?, now, CA_LIFE)?;",
        "unit",
    ),
    (
        "provision: a certificate is valid only from when it was made",
        "src/lib.rs",
        "    params.not_before = now.checked_sub(offset(SKEW)?).ok_or(ProvisionError::Clock)?;",
        "    params.not_before = now.checked_sub(offset(Duration::ZERO)?).ok_or(ProvisionError::Clock)?;",
        "unit",
    ),
    (
        "provision: the CA may issue CAs below it",
        "src/lib.rs",
        "    ca_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));",
        "    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);",
        "unit",
    ),
    (
        "provision: a kit's store key is its signing key",
        "src/lib.rs",
        "            store_key: random(&mut entropy)?,",
        "            store_key: signing_key,",
        "unit",
    ),
    (
        "provision: the cloud's certificate doesn't carry the cloud's name",
        "src/lib.rs",
        "            Some(kit(address.device, CLOUD_NAME, Some(&address.name), signing_key)?)",
        "            Some(kit(address.device, CLOUD_NAME, None, signing_key)?)",
    ),
    (
        "provision: the roster names no cloud though the location has one",
        "src/lib.rs",
        "        cloud: cloud.as_ref().map(|(address, _)| address.clone()),",
        "        cloud: None,",
    ),
]
