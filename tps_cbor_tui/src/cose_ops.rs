/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! COSE sign/verify/encrypt/decrypt operations on in-memory byte buffers.
//!
//! Vendors the exact `Sign1Builder`/`verify_sign1_*`/`Encrypt0Builder`/
//! `decrypt_encrypt0_with_cose_key` call sequences (including buffer-sizing
//! margins and nonce handling) already proven in
//! `tps_cose_tool/src/commands/{sign1,encrypt0}.rs`, adapted to operate on
//! `&[u8]` rather than files.

use ring::rand::{SecureRandom, SystemRandom};
use tps_cose::{
    Algorithm, CoseError, CoseKey, Curve, Ec2Y, EdDsaPrivateKey, EdDsaPublicKey, Encrypt0Builder,
    EncryptAlgorithm, Es256PrivateKey, Es256PublicKey, Payload, Sign1Builder, decode_encrypt0,
    decode_sign1, decrypt_encrypt0_with_cose_key, verify_sign1_eddsa, verify_sign1_es256,
};

fn ec2_public_bytes<'a>(k: &'a tps_cose::Ec2Key<'a>) -> Result<(&'a [u8], &'a [u8]), CoseError> {
    let y_bytes = match k.y {
        Some(Ec2Y::Bytes(b)) => b,
        _ => return Err(CoseError::InvalidKey),
    };
    Ok((k.x, y_bytes))
}

/// Sign `payload` as a COSE_Sign1 using `key` (EC2/P-256 or OKP/Ed25519).
pub fn sign(payload: &[u8], key: &CoseKey<'_>) -> Result<Vec<u8>, CoseError> {
    let payload_len = payload.len();
    let mut protected_buf = vec![0u8; 256];
    let mut sig_structure_buf = vec![0u8; payload_len + 512];
    let mut signature_buf = vec![0u8; 128];
    let mut out = vec![0u8; payload_len + 1024];

    let n = match key {
        CoseKey::Ec2(k) if k.curve == Curve::P256 => {
            let d = k.d.ok_or(CoseError::InvalidKey)?;
            let (x, y_bytes) = ec2_public_bytes(k)?;
            let priv_key = Es256PrivateKey { d, x, y: y_bytes };
            Sign1Builder::new(&[], &[])
                .algorithm(Algorithm::Es256)
                .payload(Payload::Attached(payload))
                .sign_es256(
                    &priv_key,
                    &mut protected_buf,
                    &mut sig_structure_buf,
                    &mut signature_buf,
                    &mut out,
                )?
        }
        CoseKey::Okp(k) if k.curve == Curve::Ed25519 => {
            let d = k.d.ok_or(CoseError::InvalidKey)?;
            let priv_key = EdDsaPrivateKey { d, x: k.x };
            Sign1Builder::new(&[], &[])
                .algorithm(Algorithm::EdDsa)
                .payload(Payload::Attached(payload))
                .sign_eddsa(
                    &priv_key,
                    &mut protected_buf,
                    &mut sig_structure_buf,
                    &mut signature_buf,
                    &mut out,
                )?
        }
        _ => return Err(CoseError::InvalidKey),
    };

    out.truncate(n);
    Ok(out)
}

/// Verify a COSE_Sign1 `bytes` blob using `key`, returning the payload on success.
pub fn verify(bytes: &[u8], key: &CoseKey<'_>) -> Result<Vec<u8>, CoseError> {
    let sign1 = decode_sign1(bytes)?;
    let payload = sign1.payload.ok_or(CoseError::InvalidStructure)?;

    let mut sig_structure_buf = vec![0u8; payload.len() + 512];

    match key {
        CoseKey::Ec2(k) if k.curve == Curve::P256 => {
            let (x, y_bytes) = ec2_public_bytes(k)?;
            let pub_key = Es256PublicKey { x, y: y_bytes };
            verify_sign1_es256(&sign1, &pub_key, &[], None, &mut sig_structure_buf)?;
        }
        CoseKey::Okp(k) if k.curve == Curve::Ed25519 => {
            let pub_key = EdDsaPublicKey { x: k.x };
            verify_sign1_eddsa(&sign1, &pub_key, &[], None, &mut sig_structure_buf)?;
        }
        _ => return Err(CoseError::InvalidKey),
    }

    Ok(payload.to_vec())
}

fn nonce_len(alg: EncryptAlgorithm) -> usize {
    match alg {
        EncryptAlgorithm::A128Gcm => 12,
        EncryptAlgorithm::AesCcm16_64_128
        | EncryptAlgorithm::AesCcm16_64_256
        | EncryptAlgorithm::AesCcm16_128_128
        | EncryptAlgorithm::AesCcm16_128_256 => 13,
        EncryptAlgorithm::AesCcm64_64_128
        | EncryptAlgorithm::AesCcm64_64_256
        | EncryptAlgorithm::AesCcm64_128_128
        | EncryptAlgorithm::AesCcm64_128_256 => 7,
    }
}

/// Encrypt `plaintext` as a COSE_Encrypt0 using symmetric `key`.
pub fn encrypt(plaintext: &[u8], key: &CoseKey<'_>, alg: EncryptAlgorithm) -> Result<Vec<u8>, CoseError> {
    let mut nonce = vec![0u8; nonce_len(alg)];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| CoseError::InvalidKey)?;

    let pt_len = plaintext.len();
    let mut protected_buf = vec![0u8; 256];
    let mut enc_structure_buf = vec![0u8; pt_len + 512];
    let mut ciphertext_buf = vec![0u8; pt_len + 32];
    let mut out = vec![0u8; pt_len + 1024];

    let n = Encrypt0Builder::new(&[], &[])
        .algorithm(alg)
        .payload(Payload::Attached(plaintext))
        .nonce(nonce.as_slice())
        .encrypt_with_cose_key(
            key,
            &mut protected_buf,
            &mut enc_structure_buf,
            &mut ciphertext_buf,
            &mut out,
        )?;

    out.truncate(n);
    Ok(out)
}

/// Decrypt a COSE_Encrypt0 `bytes` blob using symmetric `key`.
pub fn decrypt(bytes: &[u8], key: &CoseKey<'_>) -> Result<Vec<u8>, CoseError> {
    let encrypt0 = decode_encrypt0(bytes)?;
    let ciphertext_len = encrypt0.ciphertext.map(|c| c.len()).unwrap_or(0);

    let mut enc_structure_buf = vec![0u8; ciphertext_len + 512];
    let mut plaintext_buf = vec![0u8; ciphertext_len];

    let n = decrypt_encrypt0_with_cose_key(
        &encrypt0,
        key,
        &[],
        None,
        &mut enc_structure_buf,
        &mut plaintext_buf,
    )?;

    plaintext_buf.truncate(n);
    Ok(plaintext_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tps_cose::{KeyCommon, SymmetricKey};

    /// Minimal base64url (no padding) decoder, just enough for the fixed
    /// P-256 test vector strings below (borrowed from `tps_cose`'s own
    /// `tests/sign.rs` fixture, RFC 8152 Appendix B.1-style test key).
    fn decode_base64url(input: &str) -> Vec<u8> {
        fn val(c: u8) -> u32 {
            match c {
                b'A'..=b'Z' => (c - b'A') as u32,
                b'a'..=b'z' => (c - b'a' + 26) as u32,
                b'0'..=b'9' => (c - b'0' + 52) as u32,
                b'-' => 62,
                b'_' => 63,
                _ => panic!("invalid base64url byte"),
            }
        }
        let bytes = input.as_bytes();
        let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
        let mut chunks = bytes.chunks(4);
        for chunk in &mut chunks {
            let vals: Vec<u32> = chunk.iter().map(|&c| val(c)).collect();
            match vals.len() {
                4 => {
                    let n = (vals[0] << 18) | (vals[1] << 12) | (vals[2] << 6) | vals[3];
                    out.push((n >> 16) as u8);
                    out.push((n >> 8) as u8);
                    out.push(n as u8);
                }
                3 => {
                    let n = (vals[0] << 18) | (vals[1] << 12) | (vals[2] << 6);
                    out.push((n >> 16) as u8);
                    out.push((n >> 8) as u8);
                }
                2 => {
                    let n = (vals[0] << 18) | (vals[1] << 12);
                    out.push((n >> 16) as u8);
                }
                _ => panic!("invalid base64url length"),
            }
        }
        out
    }

    fn es256_test_key() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        // Same fixed P-256 test vector `tps_cose/tests/sign.rs` uses.
        let d = decode_base64url("V8kgd2ZBRuh2dgyVINBUqpPDr7BOMGcF22CQMIUHtNM");
        let x = decode_base64url("usWxHK2PmfnHKwXPS54m0kTcGJ90UiglWiGahtagnv8");
        let y = decode_base64url("IBOL-C3BttVivg-lSreASjpkttcsz-1rb7btKLv8EX4");
        (d, x, y)
    }

    #[test]
    fn symmetric_encrypt_decrypt_round_trip() {
        let k = [9u8; 16];
        let key = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon::default(),
            k: &k,
        });
        let plaintext = b"hello cose";
        let ciphertext = encrypt(plaintext, &key, EncryptAlgorithm::A128Gcm).unwrap();
        let decrypted = decrypt(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decrypt_with_wrong_key_fails() {
        let k1 = [9u8; 16];
        let k2 = [8u8; 16];
        let key1 = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon::default(),
            k: &k1,
        });
        let key2 = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon::default(),
            k: &k2,
        });
        let ciphertext = encrypt(b"hello", &key1, EncryptAlgorithm::A128Gcm).unwrap();
        assert!(decrypt(&ciphertext, &key2).is_err());
    }

    #[test]
    fn sign_verify_round_trip_es256() {
        let (d, x, y) = es256_test_key();
        let key = CoseKey::Ec2(tps_cose::Ec2Key {
            common: KeyCommon::default(),
            curve: Curve::P256,
            x: &x,
            y: Some(Ec2Y::Bytes(&y)),
            d: Some(&d),
        });
        let payload = b"sign me";
        let signed = sign(payload, &key).unwrap();
        let verified = verify(&signed, &key).unwrap();
        assert_eq!(verified, payload);
    }

    #[test]
    fn tampered_signature_fails_verification() {
        let (d, x, y) = es256_test_key();
        let key = CoseKey::Ec2(tps_cose::Ec2Key {
            common: KeyCommon::default(),
            curve: Curve::P256,
            x: &x,
            y: Some(Ec2Y::Bytes(&y)),
            d: Some(&d),
        });
        let mut signed = sign(b"sign me", &key).unwrap();
        let last = signed.len() - 1;
        signed[last] ^= 0xff;
        assert!(verify(&signed, &key).is_err());
    }
}
