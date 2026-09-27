//! Property tests: signed messages verify, and any change to them, or any other key, is caught.

#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code: a broken assumption should fail loudly (overflow checks are always on)"
)]

use keel_events::cose::CoseSign1;
use keel_events::keys::{
    PublicKey, Signature, SignatureAlgorithm, SignatureError, Signer, SoftwareSigner,
};
use keel_types::SeededEntropy;
use num_bigint::BigUint;
use proptest::prelude::*;
use sha2::{Digest, Sha512};

fn any_algorithm() -> impl Strategy<Value = SignatureAlgorithm> {
    prop_oneof![Just(SignatureAlgorithm::Es256), Just(SignatureAlgorithm::EdDsa)]
}

fn signer(algorithm: SignatureAlgorithm, seed: u64) -> SoftwareSigner {
    SoftwareSigner::generate(algorithm, &mut SeededEntropy::new(seed)).unwrap()
}

/// The group order of Ed25519, L = 2^252 + 27742317777372353535851937790883648493, little endian.
const ED25519_ORDER: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

proptest! {
    /// Signed messages survive encoding and verify with the signer's key.
    #[test]
    fn signed_messages_verify(
        algorithm in any_algorithm(),
        seed in any::<u64>(),
        payload in prop::collection::vec(any::<u8>(), 0..300),
    ) {
        let signer = signer(algorithm, seed);
        let message = CoseSign1::sign(payload.clone(), &signer).unwrap();
        let decoded = CoseSign1::decode(&message.encode()).unwrap();
        prop_assert_eq!(decoded.payload(), payload.as_slice());
        prop_assert_eq!(decoded.verify(signer.public_key()), Ok(()));
    }

    /// Flipping any single bit of a signed message is always caught: the bytes no longer decode,
    /// or no longer verify.
    #[test]
    fn any_bit_flip_is_caught(
        algorithm in any_algorithm(),
        seed in any::<u64>(),
        payload in prop::collection::vec(any::<u8>(), 0..100),
        position in any::<usize>(),
        bit in 0_u8..8,
    ) {
        let signer = signer(algorithm, seed);
        let mut bytes = CoseSign1::sign(payload, &signer).unwrap().encode();
        let index = position % bytes.len();
        bytes[index] ^= 1 << bit;
        let verified = CoseSign1::decode(&bytes).map(|message| message.verify(signer.public_key()));
        prop_assert!(!matches!(verified, Ok(Ok(()))), "a flipped bit at {} went unnoticed", index);
    }

    /// A message never verifies with another key, of either algorithm.
    #[test]
    fn other_keys_never_verify(
        algorithm in any_algorithm(),
        other_algorithm in any_algorithm(),
        (seed, other_seed) in (any::<u64>(), any::<u64>()).prop_filter("distinct keys", |(a, b)| a != b),
        payload in prop::collection::vec(any::<u8>(), 0..100),
    ) {
        let message = CoseSign1::sign(payload, &signer(algorithm, seed)).unwrap();
        let other = signer(other_algorithm, other_seed);
        prop_assert!(message.verify(other.public_key()).is_err());
    }

    /// Every ES256 signature has a high-S twin that is valid ECDSA; Keel rejects it, so each
    /// signature has exactly one accepted form.
    #[test]
    fn high_s_twins_are_rejected(seed in any::<u64>(), message in prop::collection::vec(any::<u8>(), 0..100)) {
        let signer = signer(SignatureAlgorithm::Es256, seed);
        let signature = signer.sign(&message).unwrap();
        let parsed = p256::ecdsa::Signature::from_slice(signature.as_bytes()).unwrap();
        let (r, s) = parsed.split_scalars();
        let twin = p256::ecdsa::Signature::from_scalars(r, -*s).unwrap();
        let twin = Signature::from_bytes(twin.to_bytes().to_vec());
        prop_assert_eq!(signer.public_key().verify(&message, &twin), Err(SignatureError::NotLowS));
        prop_assert_eq!(signer.public_key().verify(&message, &signature), Ok(()));
    }

    /// Ed25519's classic malleability, adding the group order L to S, is rejected.
    #[test]
    fn ed25519_s_plus_order_is_rejected(seed in any::<u64>(), message in prop::collection::vec(any::<u8>(), 0..100)) {
        let signer = signer(SignatureAlgorithm::EdDsa, seed);
        let signature = signer.sign(&message).unwrap();
        let mut bytes = signature.as_bytes().to_vec();
        // S is the second half, little endian: add L with carries.
        let mut carry = 0_u16;
        for (byte, order_byte) in bytes[32..].iter_mut().zip(ED25519_ORDER) {
            let sum = u16::from(*byte) + u16::from(order_byte) + carry;
            *byte = u8::try_from(sum & 0xFF).unwrap();
            carry = sum >> 8;
        }
        let malleated = Signature::from_bytes(bytes);
        prop_assert_eq!(signer.public_key().verify(&message, &malleated), Err(SignatureError::Invalid));
    }

    /// Public keys survive a round trip through their canonical bytes, with the same identifier.
    #[test]
    fn public_keys_round_trip(algorithm in any_algorithm(), seed in any::<u64>()) {
        let key = signer(algorithm, seed).public_key().clone();
        let bytes = key.to_bytes();
        let parsed = match algorithm {
            SignatureAlgorithm::Es256 => PublicKey::es256_from_sec1(&bytes),
            SignatureAlgorithm::EdDsa => PublicKey::ed25519_from_bytes(&bytes),
        }
        .unwrap();
        prop_assert_eq!(parsed.key_id(), key.key_id());
        prop_assert_eq!(parsed, key);
    }
}

proptest! {
    /// An Ed25519 "signature" whose R is the identity point, a point of small order. With the
    /// signer's secret scalar a and S = k·a mod L, it satisfies the lax verification equation for
    /// any message (and lax verification is confirmed to accept it); strict verification, which
    /// Keel uses, rejects it.
    #[test]
    fn ed25519_small_order_r_is_rejected(
        seed in any::<[u8; 32]>(),
        message in prop::collection::vec(any::<u8>(), 0..100),
    ) {
        let signer = SoftwareSigner::from_secret(SignatureAlgorithm::EdDsa, &seed).unwrap();
        let public = signer.public_key().to_bytes();
        let order = BigUint::from_bytes_le(&ED25519_ORDER);
        // The secret scalar: the clamped first half of SHA-512(seed) (RFC 8032 §5.1.5).
        let mut scalar = Sha512::digest(seed)[..32].to_vec();
        scalar[0] &= 0xF8;
        scalar[31] &= 0x7F;
        scalar[31] |= 0x40;
        let secret = BigUint::from_bytes_le(&scalar);
        let mut identity = [0_u8; 32];
        identity[0] = 1;
        let challenge = Sha512::digest([&identity[..], &public, &message].concat());
        let k = BigUint::from_bytes_le(&challenge) % &order;
        let mut s = ((k * secret) % &order).to_bytes_le();
        s.resize(32, 0);
        let forged: Vec<u8> = identity.iter().chain(&s).copied().collect();

        let lax_key = ed25519_dalek::VerifyingKey::from_bytes(&public.clone().try_into().unwrap()).unwrap();
        let forged_signature = ed25519_dalek::Signature::from_bytes(&forged.clone().try_into().unwrap());
        prop_assert!(
            ed25519_dalek::Verifier::verify(&lax_key, &message, &forged_signature).is_ok(),
            "the crafted signature should pass lax verification"
        );
        prop_assert_eq!(
            signer.public_key().verify(&message, &Signature::from_bytes(forged)),
            Err(SignatureError::Invalid)
        );
    }
}
