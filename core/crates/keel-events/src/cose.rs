//! COSE_Sign1: the signed wrapper around every event (RFC 9052 §4.2).
//!
//! An event is stored and transmitted as a COSE_Sign1 message whose payload is the event body,
//! signed by the origin device's key. COSE is the IETF standard for signed CBOR, so auditors can
//! verify Keel events with any conforming COSE implementation.
//!
//! Keel writes exactly one form of message, and accepts only that form:
//! - untagged: `[protected, unprotected, payload, signature]`;
//! - a protected header of exactly `{1: alg, 4: kid}`: the algorithm (ES256 or EdDSA) and the
//!   signing key's identifier;
//! - an empty unprotected header;
//! - an attached payload;
//! - canonical CBOR throughout.
//!
//! The signature covers COSE's `Sig_structure`: `["Signature1", protected, h'', payload]`.

use crate::cbor::{self, CborError, Map, Value};
use crate::keys::{
    KeyId, PublicKey, SignError, Signature, SignatureAlgorithm, SignatureError, Signer,
};

/// COSE header label of the algorithm.
const ALG: u64 = 1;
/// COSE header label of the key identifier.
const KID: u64 = 4;

/// A payload signed by one key, in COSE_Sign1 form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoseSign1 {
    protected: Vec<u8>,
    algorithm: SignatureAlgorithm,
    key_id: KeyId,
    payload: Vec<u8>,
    signature: Signature,
}

impl CoseSign1 {
    /// Signs `payload` with `signer`.
    ///
    /// The new signature is verified before it is returned, so a faulty signer (a hardware key
    /// returning a malformed signature, say) is caught here rather than by every receiver.
    ///
    /// # Errors
    /// [`SignError`] if the signer fails, or produces a signature that doesn't verify.
    pub fn sign<S: Signer + ?Sized>(payload: Vec<u8>, signer: &S) -> Result<CoseSign1, SignError> {
        let key = signer.public_key();
        let (algorithm, key_id) = (key.algorithm(), key.key_id());
        let protected = protected_header(algorithm, key_id);
        let signature = signer.sign(&to_be_signed(&protected, &payload))?;
        let message = CoseSign1 { protected, algorithm, key_id, payload, signature };
        message
            .verify(key)
            .map_err(|error| SignError(format!("signer produced a bad signature: {error}")))?;
        Ok(message)
    }

    /// The message's encoding: canonical CBOR, untagged.
    pub fn encode(&self) -> Vec<u8> {
        Value::Array(vec![
            Value::Bytes(self.protected.clone()),
            Value::Map(Map::new()),
            Value::Bytes(self.payload.clone()),
            Value::Bytes(self.signature.as_bytes().to_vec()),
        ])
        .encode()
    }

    /// Decodes a message in exactly the form Keel writes. The signature isn't checked: call
    /// [`CoseSign1::verify`] with the key the header names.
    ///
    /// # Errors
    /// [`CoseError`] if the bytes aren't canonical CBOR or aren't a COSE_Sign1 message in Keel's
    /// form.
    pub fn decode(bytes: &[u8]) -> Result<CoseSign1, CoseError> {
        let value = cbor::decode(bytes)?;
        let Some([protected, unprotected, payload, signature]) = value.as_array() else {
            return Err(CoseError::Malformed("expected an array of four items"));
        };
        let protected = protected
            .as_bytes()
            .ok_or(CoseError::Malformed("the protected header must be a byte string"))?;
        let (algorithm, key_id) = parse_protected_header(protected)?;
        if !unprotected.as_map().is_some_and(Map::is_empty) {
            return Err(CoseError::Malformed("the unprotected header must be an empty map"));
        }
        let payload =
            payload.as_bytes().ok_or(CoseError::Malformed("the payload must be attached"))?;
        let signature = signature
            .as_bytes()
            .ok_or(CoseError::Malformed("the signature must be a byte string"))?;
        Ok(CoseSign1 {
            protected: protected.to_vec(),
            algorithm,
            key_id,
            payload: payload.to_vec(),
            signature: Signature::from_bytes(signature.to_vec()),
        })
    }

    /// Verifies the signature with `key`, which must be the key the header names.
    ///
    /// # Errors
    /// [`CoseError::KeyMismatch`] if `key` has another algorithm or identifier than the header
    /// names, [`CoseError::Signature`] if the signature doesn't verify.
    pub fn verify(&self, key: &PublicKey) -> Result<(), CoseError> {
        if key.algorithm() != self.algorithm || key.key_id() != self.key_id {
            return Err(CoseError::KeyMismatch);
        }
        key.verify(&to_be_signed(&self.protected, &self.payload), &self.signature)?;
        Ok(())
    }

    /// The signature algorithm named in the protected header.
    pub const fn algorithm(&self) -> SignatureAlgorithm {
        self.algorithm
    }

    /// The signing key's identifier, from the protected header.
    pub const fn key_id(&self) -> KeyId {
        self.key_id
    }

    /// The signed payload.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The signature.
    pub const fn signature(&self) -> &Signature {
        &self.signature
    }
}

/// The encoded protected header: `{1: alg, 4: kid}`.
fn protected_header(algorithm: SignatureAlgorithm, key_id: KeyId) -> Vec<u8> {
    let entries = [
        (Value::Unsigned(ALG), Value::integer(algorithm.cose_id())),
        (Value::Unsigned(KID), Value::from(key_id.as_bytes().as_slice())),
    ];
    // The labels are distinct, so this can't fail.
    Value::Map(Map::from_entries(entries).unwrap_or_default()).encode()
}

fn parse_protected_header(bytes: &[u8]) -> Result<(SignatureAlgorithm, KeyId), CoseError> {
    let header = cbor::decode(bytes)?;
    let entries: Vec<(&Value, &Value)> = header
        .as_map()
        .ok_or(CoseError::Malformed("the protected header must be a map"))?
        .iter()
        .collect();
    // Canonical order puts the algorithm (label 1) before the key identifier (label 4).
    let [(alg_label, alg), (kid_label, kid)] = entries.as_slice() else {
        return Err(CoseError::Malformed("the protected header must be exactly {1: alg, 4: kid}"));
    };
    if alg_label.as_u64() != Some(ALG) || kid_label.as_u64() != Some(KID) {
        return Err(CoseError::Malformed("the protected header must be exactly {1: alg, 4: kid}"));
    }
    let alg = alg.as_i64().ok_or(CoseError::Malformed("the algorithm must be an integer"))?;
    let algorithm =
        SignatureAlgorithm::from_cose_id(alg).ok_or(CoseError::UnsupportedAlgorithm(alg))?;
    let kid = kid
        .as_bytes()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .ok_or(CoseError::Malformed("the key identifier must be 32 bytes"))?;
    Ok((algorithm, KeyId::from_bytes(kid)))
}

/// The bytes a COSE_Sign1 signature covers: `["Signature1", protected, h'', payload]`.
fn to_be_signed(protected: &[u8], payload: &[u8]) -> Vec<u8> {
    Value::Array(vec![
        Value::from("Signature1"),
        Value::from(protected),
        Value::Bytes(Vec::new()),
        Value::from(payload),
    ])
    .encode()
}

/// Why a COSE_Sign1 message was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CoseError {
    /// The bytes aren't canonical CBOR.
    #[error(transparent)]
    Cbor(#[from] CborError),
    /// Canonical CBOR, but not a COSE_Sign1 message in Keel's form.
    #[error("not a COSE_Sign1 message in Keel's form: {0}")]
    Malformed(&'static str),
    /// An algorithm Keel doesn't accept.
    #[error("unsupported COSE algorithm {0}")]
    UnsupportedAlgorithm(i64),
    /// The key's algorithm or identifier differs from the header's.
    #[error("the key doesn't match the message's algorithm and key identifier")]
    KeyMismatch,
    /// The signature doesn't verify.
    #[error(transparent)]
    Signature(#[from] SignatureError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::SoftwareSigner;

    fn signer(algorithm: SignatureAlgorithm) -> SoftwareSigner {
        SoftwareSigner::from_secret(algorithm, &[7; 32]).unwrap()
    }

    #[test]
    fn sign_encode_decode_verify() {
        for algorithm in [SignatureAlgorithm::Es256, SignatureAlgorithm::EdDsa] {
            let signer = signer(algorithm);
            let message = CoseSign1::sign(b"an event body".to_vec(), &signer).unwrap();
            let decoded = CoseSign1::decode(&message.encode()).unwrap();
            assert_eq!(decoded, message);
            assert_eq!(decoded.verify(signer.public_key()), Ok(()));
            assert_eq!(decoded.payload(), b"an event body");
            assert_eq!(decoded.algorithm(), algorithm);
            assert_eq!(decoded.key_id(), signer.public_key().key_id());
        }
    }

    #[test]
    fn verification_needs_the_named_key() {
        let signer = signer(SignatureAlgorithm::EdDsa);
        let message = CoseSign1::sign(b"payload".to_vec(), &signer).unwrap();
        let other = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &[8; 32]).unwrap();
        assert_eq!(message.verify(other.public_key()), Err(CoseError::KeyMismatch));
        let es256 = self::signer(SignatureAlgorithm::Es256);
        assert_eq!(message.verify(es256.public_key()), Err(CoseError::KeyMismatch));
    }

    #[test]
    fn a_faulty_signer_is_caught_when_signing() {
        struct Faulty(SoftwareSigner);
        impl Signer for Faulty {
            fn public_key(&self) -> &PublicKey {
                self.0.public_key()
            }
            fn sign(&self, _message: &[u8]) -> Result<Signature, SignError> {
                self.0.sign(b"something else")
            }
        }
        let faulty = Faulty(signer(SignatureAlgorithm::Es256));
        let error = CoseSign1::sign(b"payload".to_vec(), &faulty).unwrap_err();
        assert!(error.0.contains("bad signature"), "{error}");
    }

    #[test]
    fn only_keels_exact_form_is_accepted() {
        let signer = signer(SignatureAlgorithm::EdDsa);
        let message = CoseSign1::sign(b"payload".to_vec(), &signer).unwrap();
        let protected = Value::Bytes(message.protected.clone());
        let payload = Value::from(b"payload".as_slice());
        let signature = Value::Bytes(message.signature.as_bytes().to_vec());
        let empty = Value::Map(Map::new());
        let build = |items: Vec<Value>| Value::Array(items).encode();

        // Tagged COSE_Sign1 (tag 18): tags aren't part of Keel's CBOR subset.
        let mut tagged = vec![0xD2];
        tagged.extend(message.encode());
        assert!(matches!(CoseSign1::decode(&tagged), Err(CoseError::Cbor(_))));
        // Detached payload.
        let detached =
            build(vec![protected.clone(), empty.clone(), Value::Null, signature.clone()]);
        assert!(matches!(CoseSign1::decode(&detached), Err(CoseError::Malformed(_))));
        // A non-empty unprotected header.
        let unprotected = Value::Map(
            Map::from_entries([(Value::Unsigned(4), Value::from(b"kid".as_slice()))]).unwrap(),
        );
        let with_unprotected =
            build(vec![protected.clone(), unprotected, payload.clone(), signature.clone()]);
        assert!(matches!(CoseSign1::decode(&with_unprotected), Err(CoseError::Malformed(_))));
        // An extra protected header parameter (content type, label 3).
        let extra = Value::Map(
            Map::from_entries([
                (Value::Unsigned(1), Value::integer(-8)),
                (Value::Unsigned(3), Value::Unsigned(60)),
                (Value::Unsigned(4), Value::from(message.key_id.as_bytes().as_slice())),
            ])
            .unwrap(),
        );
        let with_extra = build(vec![
            Value::Bytes(extra.encode()),
            empty.clone(),
            payload.clone(),
            signature.clone(),
        ]);
        assert!(matches!(CoseSign1::decode(&with_extra), Err(CoseError::Malformed(_))));
        // An unsupported algorithm (ES384, −35).
        let es384 = Value::Map(
            Map::from_entries([
                (Value::Unsigned(1), Value::integer(-35)),
                (Value::Unsigned(4), Value::from(message.key_id.as_bytes().as_slice())),
            ])
            .unwrap(),
        );
        let with_es384 =
            build(vec![Value::Bytes(es384.encode()), empty.clone(), payload, signature]);
        assert_eq!(CoseSign1::decode(&with_es384), Err(CoseError::UnsupportedAlgorithm(-35)));
        // Three items instead of four.
        let short = build(vec![protected, empty.clone(), empty]);
        assert!(matches!(CoseSign1::decode(&short), Err(CoseError::Malformed(_))));
    }
}

#[cfg(test)]
mod interop {
    //! Known-answer tests shared with an independent COSE implementation (Python's pycose).

    use super::*;
    use crate::keys::SoftwareSigner;

    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks(2)
            .map(|pair| u8::from_str_radix(core::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn signer(algorithm: SignatureAlgorithm) -> SoftwareSigner {
        SoftwareSigner::from_secret(algorithm, &[7; 32]).unwrap()
    }

    /// Keel's exact output for fixed keys (both algorithms sign deterministically). pycose
    /// verified these messages, and computed the same key identifiers independently.
    #[test]
    fn keel_messages_verified_by_pycose() {
        let expected = [
            (
                SignatureAlgorithm::Es256,
                "845826a20126045820fd65f050c9c3b53337cf17eee589eb11a81a91c55ac10aa48361c0508d959e4da04f6b65656c206576656e7420626f64795840a0d38afcb008a2eccdfc13f8c63a08a955b55c2ce18d660a2cb493afb827747d723d01524e6295362be280813293de337965965650633fbe4a3dbeb78b366d01",
            ),
            (
                SignatureAlgorithm::EdDsa,
                "845826a20127045820cbf65c8afcb5f69056927170cfe6acc226f1e3182e0aac3b83245e746beb0e6aa04f6b65656c206576656e7420626f647958409cf3501bf9d90f7e2d96934603e6b738272b8627a3bdfd7f94efc37d4710add3718e7a9dd603ab7683d58297ddc28cc3f5fef66dbfa111f37ccc64a34bf0b90d",
            ),
        ];
        for (algorithm, expected) in expected {
            let message = CoseSign1::sign(b"keel event body".to_vec(), &signer(algorithm)).unwrap();
            assert_eq!(message.encode(), hex(expected), "{algorithm}");
        }
    }

    /// Messages signed by pycose with the same keys pass Keel's strict decoding and verification.
    #[test]
    fn pycose_messages_verified_by_keel() {
        let messages = [
            (
                SignatureAlgorithm::Es256,
                "845826a20126045820fd65f050c9c3b53337cf17eee589eb11a81a91c55ac10aa48361c0508d959e4da04e6d616465206279207079636f73655840683fc338a06ec3bd4e1c6d8cab5e74e1281ec7cfa03c0e8f3e52b806ef0a97692e0e2b0f905e72303b2ff738b136ef230d0d92b3b6d90ba3bf395c013c467e0f",
            ),
            (
                SignatureAlgorithm::EdDsa,
                "845826a20127045820cbf65c8afcb5f69056927170cfe6acc226f1e3182e0aac3b83245e746beb0e6aa04e6d616465206279207079636f736558400e833f7da6d4f6687ecd4e19cf8de4dd7172d291cc684e836bc8d6f3ade6d0c32544598450340f29828c2215dd0b7920af8745d2431539bf57ab201500d5870c",
            ),
        ];
        for (algorithm, encoded) in messages {
            let message = CoseSign1::decode(&hex(encoded)).unwrap();
            assert_eq!(message.algorithm(), algorithm);
            assert_eq!(message.payload(), b"made by pycose");
            assert_eq!(message.verify(signer(algorithm).public_key()), Ok(()));
            assert_eq!(message.encode(), hex(encoded), "canonical: re-encodes identically");
        }
    }
}
