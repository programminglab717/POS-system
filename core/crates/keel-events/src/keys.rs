//! Keys and signatures: how devices sign their events, and how anyone verifies them.
//!
//! Devices sign with **ES256** (ECDSA on the P-256 curve with SHA-256), the algorithm that the
//! secure hardware of phones, tablets and PCs provides (Secure Enclave, StrongBox, TPM). Where no
//! secure hardware exists, **EdDSA** (Ed25519) is the software fallback. Both are identified by
//! their COSE algorithm numbers (RFC 9053), and both produce 64-byte signatures.
//!
//! Verification is strict, so each signature has exactly one accepted encoding: ES256 signatures
//! must be in "low S" form, and EdDSA signatures are checked with Ed25519's strict rules
//! (canonical scalars, no small-order points). Replicas therefore store byte-identical logs.

use core::fmt;

use keel_types::{Entropy, EntropyError};
use p256::ecdsa::signature::{Signer as _, Verifier as _};

use crate::cbor::{Map, Value};
use crate::hash::{sha256, write_hex};

/// A signature algorithm accepted for events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SignatureAlgorithm {
    /// ECDSA with P-256 and SHA-256: COSE "ES256". The algorithm of secure hardware.
    Es256,
    /// Ed25519: COSE "EdDSA". The software fallback.
    EdDsa,
}

impl SignatureAlgorithm {
    /// The COSE algorithm number: −7 for ES256, −8 for EdDSA.
    pub const fn cose_id(self) -> i64 {
        match self {
            SignatureAlgorithm::Es256 => -7,
            SignatureAlgorithm::EdDsa => -8,
        }
    }

    /// The algorithm with COSE number `id`, if Keel accepts it.
    pub const fn from_cose_id(id: i64) -> Option<SignatureAlgorithm> {
        match id {
            -7 => Some(SignatureAlgorithm::Es256),
            -8 => Some(SignatureAlgorithm::EdDsa),
            _ => None,
        }
    }
}

impl fmt::Display for SignatureAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SignatureAlgorithm::Es256 => "ES256",
            SignatureAlgorithm::EdDsa => "EdDSA",
        })
    }
}

/// A public key that verifies event signatures.
#[derive(Clone, PartialEq, Eq)]
pub struct PublicKey(PublicKeyInner);

#[derive(Clone, PartialEq, Eq)]
enum PublicKeyInner {
    Es256(p256::ecdsa::VerifyingKey),
    EdDsa(ed25519_dalek::VerifyingKey),
}

impl PublicKey {
    /// An ES256 public key from its SEC1 encoding, compressed (33 bytes) or uncompressed
    /// (65 bytes), as secure hardware exports it.
    ///
    /// # Errors
    /// [`KeyError::InvalidPublicKey`] if the bytes aren't a point on the P-256 curve.
    pub fn es256_from_sec1(bytes: &[u8]) -> Result<PublicKey, KeyError> {
        p256::ecdsa::VerifyingKey::from_sec1_bytes(bytes)
            .map(|key| PublicKey(PublicKeyInner::Es256(key)))
            .map_err(|_| KeyError::InvalidPublicKey(SignatureAlgorithm::Es256))
    }

    /// An EdDSA (Ed25519) public key from its 32 bytes.
    ///
    /// # Errors
    /// [`KeyError::InvalidPublicKey`] if the bytes aren't a valid Ed25519 point, or are a weak
    /// (small-order) point, which could validate forged signatures.
    pub fn ed25519_from_bytes(bytes: &[u8]) -> Result<PublicKey, KeyError> {
        let invalid = KeyError::InvalidPublicKey(SignatureAlgorithm::EdDsa);
        let bytes = <&[u8; 32]>::try_from(bytes).map_err(|_| invalid.clone())?;
        let key = ed25519_dalek::VerifyingKey::from_bytes(bytes).map_err(|_| invalid.clone())?;
        if key.is_weak() {
            return Err(invalid);
        }
        Ok(PublicKey(PublicKeyInner::EdDsa(key)))
    }

    /// The algorithm the key verifies.
    pub const fn algorithm(&self) -> SignatureAlgorithm {
        match self.0 {
            PublicKeyInner::Es256(_) => SignatureAlgorithm::Es256,
            PublicKeyInner::EdDsa(_) => SignatureAlgorithm::EdDsa,
        }
    }

    /// The key's canonical bytes: the uncompressed SEC1 point (65 bytes) for ES256, the 32-byte
    /// point for EdDSA.
    pub fn to_bytes(&self) -> Vec<u8> {
        match &self.0 {
            PublicKeyInner::Es256(key) => key.to_sec1_point(false).as_bytes().to_vec(),
            PublicKeyInner::EdDsa(key) => key.to_bytes().to_vec(),
        }
    }

    /// The key's COSE_Key representation (RFC 9052 §7) with only its required parameters:
    /// `{1: kty, -1: crv, -2: x}` plus `-3: y` for P-256.
    pub fn cose_key(&self) -> Map {
        let int = Value::integer;
        let entries = match &self.0 {
            PublicKeyInner::Es256(key) => {
                let point = key.to_sec1_point(false);
                // An uncompressed SEC1 point is 0x04, then x and y, 32 bytes each. A valid key's
                // point always has that form.
                let coordinates = point.as_bytes().get(1..).and_then(|xy| xy.split_at_checked(32));
                let (x, y) = coordinates.unwrap_or_default();
                vec![
                    (int(1), int(2)),  // kty: EC2
                    (int(-1), int(1)), // crv: P-256
                    (int(-2), Value::from(x)),
                    (int(-3), Value::from(y)),
                ]
            }
            PublicKeyInner::EdDsa(key) => vec![
                (int(1), int(1)),  // kty: OKP
                (int(-1), int(6)), // crv: Ed25519
                (int(-2), Value::from(key.to_bytes().to_vec())),
            ],
        };
        // The labels are distinct, so this can't fail.
        Map::from_entries(entries).unwrap_or_default()
    }

    /// The key's identifier: SHA-256 of the canonical encoding of [`PublicKey::cose_key`]. This
    /// follows the construction of COSE Key Thumbprints (RFC 9679).
    pub fn key_id(&self) -> KeyId {
        KeyId(sha256(&Value::Map(self.cose_key()).encode()))
    }

    /// Verifies `signature` over `message`.
    ///
    /// # Errors
    /// [`SignatureError::WrongLength`] unless the signature has 64 bytes,
    /// [`SignatureError::NotLowS`] for an ES256 signature not in low-S form, and
    /// [`SignatureError::Invalid`] if it doesn't verify.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), SignatureError> {
        let bytes = signature.as_bytes();
        let wrong_length =
            SignatureError::WrongLength { algorithm: self.algorithm(), length: bytes.len() };
        match &self.0 {
            PublicKeyInner::Es256(key) => {
                if bytes.len() != SIGNATURE_LENGTH {
                    return Err(wrong_length);
                }
                let signature = p256::ecdsa::Signature::from_slice(bytes)
                    .map_err(|_| SignatureError::Invalid)?;
                if signature.normalize_s() != signature {
                    return Err(SignatureError::NotLowS);
                }
                key.verify(message, &signature).map_err(|_| SignatureError::Invalid)
            }
            PublicKeyInner::EdDsa(key) => {
                let bytes = <&[u8; 64]>::try_from(bytes).map_err(|_| wrong_length)?;
                let signature = ed25519_dalek::Signature::from_bytes(bytes);
                key.verify_strict(message, &signature).map_err(|_| SignatureError::Invalid)
            }
        }
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({} ", self.algorithm())?;
        write_hex(f, &self.to_bytes())?;
        f.write_str(")")
    }
}

/// The length of ES256 and EdDSA signatures.
const SIGNATURE_LENGTH: usize = 64;

/// Identifies a public key: SHA-256 of its canonical COSE_Key encoding.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId([u8; 32]);

impl KeyId {
    /// A key identifier from its 32 bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> KeyId {
        KeyId(bytes)
    }

    /// The 32 bytes of the identifier.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for KeyId {
    /// Lowercase hexadecimal.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_hex(f, &self.0)
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyId({self})")
    }
}

/// A signature in COSE form: for ES256, the 32-byte integers r and s; for EdDSA, R and S.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Signature(Vec<u8>);

impl Signature {
    /// A signature from its bytes. Nothing is checked until it is verified.
    pub fn from_bytes(bytes: Vec<u8>) -> Signature {
        Signature(bytes)
    }

    /// The signature's bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// An ES256 signature in COSE form, from the ASN.1 DER encoding that secure hardware
    /// returns (Android Keystore, Apple Secure Enclave). The result is normalized to low S.
    ///
    /// # Errors
    /// [`SignatureError::Invalid`] if the bytes aren't a DER-encoded P-256 ECDSA signature.
    pub fn es256_from_der(der: &[u8]) -> Result<Signature, SignatureError> {
        let signature =
            p256::ecdsa::Signature::from_der(der).map_err(|_| SignatureError::Invalid)?;
        Ok(Signature(signature.normalize_s().to_bytes().to_vec()))
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Signature(")?;
        write_hex(f, &self.0)?;
        f.write_str(")")
    }
}

/// Signs event bodies on behalf of a device.
///
/// On production devices the key lives in secure hardware, and the platform shell implements
/// this trait. [`SoftwareSigner`] keeps its key in memory: for tests, simulations, and devices
/// without secure hardware.
pub trait Signer {
    /// The public half of the signing key.
    fn public_key(&self) -> &PublicKey;

    /// Signs `message`, returning the signature in COSE form. ES256 signatures must be in low-S
    /// form ([`Signature::es256_from_der`] converts what hardware returns).
    ///
    /// # Errors
    /// [`SignError`] if the key couldn't sign, for example because secure hardware refused.
    fn sign(&self, message: &[u8]) -> Result<Signature, SignError>;
}

/// A signing key held in memory.
pub struct SoftwareSigner {
    key: SigningKey,
    public_key: PublicKey,
}

enum SigningKey {
    Es256(p256::ecdsa::SigningKey),
    EdDsa(ed25519_dalek::SigningKey),
}

impl SoftwareSigner {
    /// A signer from a 32-byte secret: a P-256 scalar for ES256, an Ed25519 seed for EdDSA.
    ///
    /// # Errors
    /// [`KeyError::InvalidSecretKey`] if the bytes aren't a valid P-256 scalar (zero, or not
    /// below the curve order).
    pub fn from_secret(
        algorithm: SignatureAlgorithm,
        secret: &[u8; 32],
    ) -> Result<SoftwareSigner, KeyError> {
        match algorithm {
            SignatureAlgorithm::Es256 => {
                let key = p256::ecdsa::SigningKey::from_slice(secret)
                    .map_err(|_| KeyError::InvalidSecretKey)?;
                let public_key = PublicKey(PublicKeyInner::Es256(*key.verifying_key()));
                Ok(SoftwareSigner { key: SigningKey::Es256(key), public_key })
            }
            SignatureAlgorithm::EdDsa => {
                let key = ed25519_dalek::SigningKey::from_bytes(secret);
                let public_key = PublicKey(PublicKeyInner::EdDsa(key.verifying_key()));
                Ok(SoftwareSigner { key: SigningKey::EdDsa(key), public_key })
            }
        }
    }

    /// A signer with a fresh key drawn from `entropy`.
    ///
    /// # Errors
    /// [`KeyError::Entropy`] if the entropy source fails.
    pub fn generate<E: Entropy>(
        algorithm: SignatureAlgorithm,
        entropy: &mut E,
    ) -> Result<SoftwareSigner, KeyError> {
        loop {
            let mut secret = [0_u8; 32];
            for chunk in secret.chunks_mut(8) {
                chunk
                    .copy_from_slice(&entropy.next_u64().map_err(KeyError::Entropy)?.to_be_bytes());
            }
            // A P-256 secret must be below the curve order: this fails with probability about
            // 2^-32, and then another secret is drawn.
            match SoftwareSigner::from_secret(algorithm, &secret) {
                Err(KeyError::InvalidSecretKey) => {}
                result => return result,
            }
        }
    }
}

impl Signer for SoftwareSigner {
    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    fn sign(&self, message: &[u8]) -> Result<Signature, SignError> {
        match &self.key {
            SigningKey::Es256(key) => {
                // Deterministic (RFC 6979), then normalized to low S.
                let signature: p256::ecdsa::Signature =
                    key.try_sign(message).map_err(|error| SignError(error.to_string()))?;
                Ok(Signature(signature.normalize_s().to_bytes().to_vec()))
            }
            SigningKey::EdDsa(key) => {
                let signature =
                    key.try_sign(message).map_err(|error| SignError(error.to_string()))?;
                Ok(Signature(signature.to_bytes().to_vec()))
            }
        }
    }
}

impl fmt::Debug for SoftwareSigner {
    /// Shows the public key only: never the secret.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SoftwareSigner")
            .field("public_key", &self.public_key)
            .finish_non_exhaustive()
    }
}

/// Errors from keys.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KeyError {
    /// Bytes that aren't a valid public key for the algorithm.
    #[error("invalid {0} public key")]
    InvalidPublicKey(SignatureAlgorithm),
    /// Bytes that aren't a valid secret key.
    #[error("invalid secret key")]
    InvalidSecretKey,
    /// The entropy source failed while generating a key.
    #[error(transparent)]
    Entropy(EntropyError),
}

/// Why a signature was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SignatureError {
    /// A signature of the wrong length for its algorithm.
    #[error("{algorithm} signatures have 64 bytes, not {length}")]
    WrongLength {
        /// The algorithm of the verifying key.
        algorithm: SignatureAlgorithm,
        /// The signature's length.
        length: usize,
    },
    /// An ES256 signature whose s is in the upper half of its range: valid ECDSA, but not the
    /// canonical form Keel accepts.
    #[error("ES256 signature is not in low-S form")]
    NotLowS,
    /// A signature that doesn't verify.
    #[error("signature does not verify")]
    Invalid,
}

/// A signer failed to sign.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("signing failed: {0}")]
pub struct SignError(pub String);

#[cfg(test)]
mod tests {
    use keel_types::SeededEntropy;

    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let text: String = text.split_whitespace().collect();
        text.as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn secret(text: &str) -> [u8; 32] {
        hex(text).try_into().unwrap()
    }

    /// RFC 8032 §7.1, tests 1 to 3 (confirmed with OpenSSL).
    #[test]
    fn ed25519_rfc_8032_vectors() {
        let vectors = [
            (
                "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60",
                "",
                "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a",
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
            ),
            (
                "4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb",
                "72",
                "3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c",
                "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
            ),
            (
                "c5aa8df43f9f837bedb7442f31dcb7b166d38535076f094b85ce3a2e0b4458f7",
                "af82",
                "fc51cd8e6218a1a38da47ed00230f0580816ed13ba3303ac5deb911548908025",
                "6291d657deec24024827e69c3abe01a30ce548a284743a445e3680d7db5ac3ac18ff9b538d16f290ae67f760984dc6594a7c15e9716ed28dc027beceea1ec40a",
            ),
        ];
        for (secret_hex, message, public, signature) in vectors {
            let signer =
                SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &secret(secret_hex))
                    .unwrap();
            assert_eq!(signer.public_key().to_bytes(), hex(public));
            let signed = signer.sign(&hex(message)).unwrap();
            assert_eq!(signed.as_bytes(), hex(signature));
            let key = PublicKey::ed25519_from_bytes(&hex(public)).unwrap();
            assert_eq!(key.verify(&hex(message), &signed), Ok(()));
            assert_eq!(key.verify(b"another message", &signed), Err(SignatureError::Invalid));
        }
    }

    /// RFC 6979 A.2.5 (P-256, SHA-256, message "sample"). The RFC's signature has a high s; the
    /// signer returns the equivalent low-S form, n − s.
    #[test]
    fn es256_rfc_6979_vector_in_low_s_form() {
        let signer = SoftwareSigner::from_secret(
            SignatureAlgorithm::Es256,
            &secret("c9afa9d845ba75166b5c215767b1d6934e50c3db36e89b127b8a622b120f6721"),
        )
        .unwrap();
        assert_eq!(
            signer.public_key().to_bytes(),
            hex("04 60fed4ba255a9d31c961eb74c6356d68c049b8923b61fa6ce669622e60f29fb6
                    7903fe1008b8bc99a41ae9e95628bc64f2f1b20c2d7e9f5177a3c294d4462299")
        );
        let high_s = Signature::from_bytes(hex(
            "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716
             f7cb1c942d657c41d436c7a1b6e29f65f3e900dbb9aff4064dc4ab2f843acda8",
        ));
        // n − s, computed with Python's arbitrary-precision integers.
        let low_s = Signature::from_bytes(hex(
            "efd48b2aacb6a8fd1140dd9cd45e81d69d2c877b56aaf991c34d0ea84eaf3716
             0834e36ad29a83bf2bc9385e491d6099c8fdf9d1ed67aa7ea5f51f93782857a9",
        ));
        assert_eq!(signer.sign(b"sample").unwrap(), low_s);
        let key = signer.public_key();
        assert_eq!(key.verify(b"sample", &low_s), Ok(()));
        assert_eq!(key.verify(b"sample", &high_s), Err(SignatureError::NotLowS));
        assert_eq!(key.verify(b"example", &low_s), Err(SignatureError::Invalid));
    }

    #[test]
    fn der_signatures_from_hardware_are_normalized() {
        let signer = SoftwareSigner::from_secret(SignatureAlgorithm::Es256, &[7; 32]).unwrap();
        let signature = signer.sign(b"keel").unwrap();
        let parsed = p256::ecdsa::Signature::from_slice(signature.as_bytes()).unwrap();
        // The high-S twin, DER encoded, as hardware might return it.
        let (r, s) = parsed.split_scalars();
        let high = p256::ecdsa::Signature::from_scalars(r, -*s).unwrap();
        assert_ne!(high, parsed);
        let from_hardware = Signature::es256_from_der(high.to_der().as_bytes()).unwrap();
        assert_eq!(from_hardware, signature);
        assert_eq!(Signature::es256_from_der(&[0x30, 0x00]), Err(SignatureError::Invalid));
    }

    #[test]
    fn key_ids_follow_the_cose_key_thumbprint_construction() {
        let signer = SoftwareSigner::from_secret(
            SignatureAlgorithm::EdDsa,
            &secret("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"),
        )
        .unwrap();
        let key = signer.public_key();
        // {1: 1, -1: 6, -2: h'd75a…'}: kty OKP, crv Ed25519, x.
        let expected_encoding = [
            hex("a3 01 01 20 06 21 5820"),
            hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a"),
        ]
        .concat();
        assert_eq!(Value::Map(key.cose_key()).encode(), expected_encoding);
        assert_eq!(key.key_id().as_bytes(), &sha256(&expected_encoding));
        let es256 = SoftwareSigner::from_secret(SignatureAlgorithm::Es256, &[7; 32]).unwrap();
        let encoding = Value::Map(es256.public_key().cose_key()).encode();
        assert_eq!(encoding.get(..6), Some(hex("a4 01 02 20 01 21").as_slice()));
        assert_ne!(es256.public_key().key_id(), key.key_id());
    }

    #[test]
    fn invalid_keys_and_signatures_are_rejected() {
        assert_eq!(
            PublicKey::es256_from_sec1(&[4; 65]),
            Err(KeyError::InvalidPublicKey(SignatureAlgorithm::Es256))
        );
        assert_eq!(
            PublicKey::ed25519_from_bytes(&[1; 31]),
            Err(KeyError::InvalidPublicKey(SignatureAlgorithm::EdDsa))
        );
        // The identity point: a weak key that "verifies" forged signatures.
        let identity = hex("0100000000000000000000000000000000000000000000000000000000000000");
        assert_eq!(
            PublicKey::ed25519_from_bytes(&identity),
            Err(KeyError::InvalidPublicKey(SignatureAlgorithm::EdDsa))
        );
        // The curve order is not a valid P-256 secret.
        let order = secret("ffffffff00000000ffffffffffffffffbce6faada7179e84f3b9cac2fc632551");
        assert!(matches!(
            SoftwareSigner::from_secret(SignatureAlgorithm::Es256, &order),
            Err(KeyError::InvalidSecretKey)
        ));
        let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[3; 32]).unwrap();
        let short = Signature::from_bytes(vec![0; 63]);
        assert_eq!(
            signer.public_key().verify(b"m", &short),
            Err(SignatureError::WrongLength { algorithm: SignatureAlgorithm::EdDsa, length: 63 })
        );
    }

    #[test]
    fn generated_keys_are_deterministic_for_a_seed_and_secrets_stay_hidden() {
        let mut entropy = SeededEntropy::new(99);
        let a = SoftwareSigner::generate(SignatureAlgorithm::Es256, &mut entropy).unwrap();
        let mut entropy = SeededEntropy::new(99);
        let b = SoftwareSigner::generate(SignatureAlgorithm::Es256, &mut entropy).unwrap();
        assert_eq!(a.public_key(), b.public_key());
        assert_eq!(a.sign(b"m").unwrap(), b.sign(b"m").unwrap());
        let debug = format!("{a:?}");
        assert!(debug.starts_with("SoftwareSigner { public_key: PublicKey(ES256 04"), "{debug}");
    }
}
