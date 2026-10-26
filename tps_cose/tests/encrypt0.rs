/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
#[cfg(feature = "ring")]
use tps_cose::decode_encrypt0;
#[cfg(feature = "ring")]
use tps_cose::{
    CoseError, CoseKey, Encrypt0Builder, EncryptAlgorithm, KeyCommon, Payload, SymmetricKey,
    decrypt_encrypt0_with_cose_key, decrypt_encrypt0_with_raw_key,
};

#[cfg(feature = "ring")]
fn decode_hex(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 2);
    let mut bytes = input.as_bytes().iter().copied();
    while let (Some(h), Some(l)) = (bytes.next(), bytes.next()) {
        let hi = hex_value(h).expect("hex high");
        let lo = hex_value(l).expect("hex low");
        out.push((hi << 4) | lo);
    }
    out
}

#[cfg(feature = "ring")]
fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(feature = "ring")]
fn split_tag(data: &[u8]) -> Result<(Option<u64>, &[u8]), &'static str> {
    if data.is_empty() {
        return Err("empty");
    }
    let initial = data[0];
    if initial >> 5 != 6 {
        return Ok((None, data));
    }
    let addl = initial & 0x1f;
    match addl {
        0..=23 => Ok((Some(addl as u64), &data[1..])),
        24 => {
            if data.len() < 2 {
                return Err("truncated");
            }
            Ok((Some(data[1] as u64), &data[2..]))
        }
        25 => {
            if data.len() < 3 {
                return Err("truncated");
            }
            Ok((
                Some(u16::from_be_bytes([data[1], data[2]]) as u64),
                &data[3..],
            ))
        }
        _ => Err("unsupported"),
    }
}

#[cfg(feature = "ring")]
fn run_round_trip(
    plaintext: &[u8],
    aad: &[u8],
    detached: bool,
    use_typed_key: bool,
) -> Result<(), CoseError> {
    let key = [0x11u8; 16];
    let nonce = [0x22u8; 12];
    let mut protected = [0u8; 64];
    let mut enc_structure = [0u8; 128];
    let mut ciphertext = [0u8; 256];
    let mut message = [0u8; 256];

    let payload = if detached {
        Payload::Detached(plaintext)
    } else {
        Payload::Attached(plaintext)
    };
    let builder = Encrypt0Builder::new(&[], &[])
        .algorithm(EncryptAlgorithm::A128Gcm)
        .nonce(&nonce)
        .payload(payload)
        .external_aad(aad);

    let msg_len = if use_typed_key {
        let cose_key = CoseKey::Symmetric(SymmetricKey::new(&key));
        builder.encrypt_with_cose_key(
            &cose_key,
            &mut protected,
            &mut enc_structure,
            &mut ciphertext,
            &mut message,
        )?
    } else {
        builder.encrypt_with_raw_key(
            &key,
            &mut protected,
            &mut enc_structure,
            &mut ciphertext,
            &mut message,
        )?
    };

    let decoded = decode_encrypt0(&message[..msg_len])?;
    let mut plaintext_out = [0u8; 256];
    let detached_ciphertext = if detached {
        decoded
            .ciphertext
            .is_none()
            .then_some(&ciphertext[..plaintext.len() + 16])
    } else {
        None
    };
    let plain_len = if use_typed_key {
        let cose_key = CoseKey::Symmetric(SymmetricKey::new(&key));
        decrypt_encrypt0_with_cose_key(
            &decoded,
            &cose_key,
            aad,
            detached_ciphertext,
            &mut enc_structure,
            &mut plaintext_out,
        )?
    } else {
        decrypt_encrypt0_with_raw_key(
            &decoded,
            &key,
            aad,
            detached_ciphertext,
            &mut enc_structure,
            &mut plaintext_out,
        )?
    };
    assert_eq!(&plaintext_out[..plain_len], plaintext);
    Ok(())
}

#[cfg(feature = "ring")]
fn run_round_trip_ccm(
    plaintext: &[u8],
    aad: &[u8],
    detached: bool,
    use_typed_key: bool,
) -> Result<(), CoseError> {
    let key = [0x11u8; 16];
    let nonce = [0x22u8; 13];
    let mut protected = [0u8; 64];
    let mut enc_structure = [0u8; 128];
    let mut ciphertext = [0u8; 256];
    let mut message = [0u8; 256];

    let payload = if detached {
        Payload::Detached(plaintext)
    } else {
        Payload::Attached(plaintext)
    };
    let builder = Encrypt0Builder::new(&[], &[])
        .algorithm(EncryptAlgorithm::AesCcm16_64_128)
        .nonce(&nonce)
        .payload(payload)
        .external_aad(aad);

    let msg_len = if use_typed_key {
        let cose_key = CoseKey::Symmetric(SymmetricKey::new(&key));
        builder.encrypt_with_cose_key(
            &cose_key,
            &mut protected,
            &mut enc_structure,
            &mut ciphertext,
            &mut message,
        )?
    } else {
        builder.encrypt_with_raw_key(
            &key,
            &mut protected,
            &mut enc_structure,
            &mut ciphertext,
            &mut message,
        )?
    };

    let decoded = decode_encrypt0(&message[..msg_len])?;
    let mut plaintext_out = [0u8; 256];
    let detached_ciphertext = if detached {
        decoded
            .ciphertext
            .is_none()
            .then_some(&ciphertext[..plaintext.len() + 8])
    } else {
        None
    };
    let plain_len = if use_typed_key {
        let cose_key = CoseKey::Symmetric(SymmetricKey::new(&key));
        decrypt_encrypt0_with_cose_key(
            &decoded,
            &cose_key,
            aad,
            detached_ciphertext,
            &mut enc_structure,
            &mut plaintext_out,
        )?
    } else {
        decrypt_encrypt0_with_raw_key(
            &decoded,
            &key,
            aad,
            detached_ciphertext,
            &mut enc_structure,
            &mut plaintext_out,
        )?
    };
    assert_eq!(&plaintext_out[..plain_len], plaintext);
    Ok(())
}

#[cfg(feature = "ring")]
#[test]
fn encrypt0_a128gcm_attached_and_detached_round_trip() {
    run_round_trip(b"This is the content.", b"", false, false).unwrap();
    run_round_trip(b"This is the content.", b"\x00\x11\x22\x33", true, false).unwrap();
    run_round_trip(b"", b"\xaa\xbb", false, false).unwrap();
    run_round_trip(b"typed-key", b"aad", false, true).unwrap();
}

#[cfg(feature = "ring")]
#[test]
fn encrypt0_a128gcm_property_style_lengths() {
    for len in 0..96usize {
        let mut data = [0u8; 96];
        for (i, b) in data.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(3).wrapping_add(1);
        }
        let plaintext = &data[..len];
        let aad = &data[..len % 17];
        run_round_trip(plaintext, aad, len % 2 == 0, len % 3 == 0).unwrap();
    }
}

#[cfg(feature = "ring")]
#[test]
fn encrypt0_aes_ccm_16_64_128_attached_and_detached_round_trip() {
    run_round_trip_ccm(b"This is the content.", b"", false, false).unwrap();
    run_round_trip_ccm(b"This is the content.", b"\x00\x11\x22\x33", true, false).unwrap();
    run_round_trip_ccm(b"", b"\xaa\xbb", false, false).unwrap();
    run_round_trip_ccm(b"typed-key", b"aad", false, true).unwrap();
}

#[cfg(feature = "ring")]
#[test]
fn decrypt_fails_on_wrong_external_aad() {
    let key = [0x11u8; 16];
    let nonce = [0x22u8; 12];
    let mut protected = [0u8; 64];
    let mut enc_structure = [0u8; 128];
    let mut ciphertext = [0u8; 128];
    let mut message = [0u8; 128];
    let builder = Encrypt0Builder::new(&[], &[])
        .algorithm(EncryptAlgorithm::A128Gcm)
        .nonce(&nonce)
        .payload(Payload::Attached(b"abc"))
        .external_aad(b"good");
    let msg_len = builder
        .encrypt_with_raw_key(
            &key,
            &mut protected,
            &mut enc_structure,
            &mut ciphertext,
            &mut message,
        )
        .unwrap();
    let decoded = decode_encrypt0(&message[..msg_len]).unwrap();
    let mut plain = [0u8; 16];
    let err =
        decrypt_encrypt0_with_raw_key(&decoded, &key, b"bad", None, &mut enc_structure, &mut plain)
            .expect_err("aad mismatch should fail");
    assert_eq!(err, CoseError::InvalidSignature);
}

#[cfg(feature = "ring")]
#[test]
fn rfc9052_encrypt0_aes_ccm_examples_decrypt() {
    // RFC 9052 C.4.1 (AES-CCM-16-64-128, tag 16)
    let c41 = decode_hex(
        "d08343a1010aa1054d89f52f65a1c580933b5261a78c581c5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569",
    );
    let (tag, untagged) = split_tag(&c41).expect("split c4.1");
    assert_eq!(tag, Some(16));
    let decoded = decode_encrypt0(untagged).expect("decode c4.1");
    let key = decode_hex("849B5786457C1491BE3A76DCEA6C4271");
    let mut enc_structure = [0u8; 128];
    let mut plaintext = [0u8; 64];
    let plain_len = decrypt_encrypt0_with_raw_key(
        &decoded,
        &key,
        b"",
        None,
        &mut enc_structure,
        &mut plaintext,
    )
    .expect("decrypt c4.1");
    assert_eq!(&plaintext[..plain_len], b"This is the content.");

    // RFC 9052 C.4.2 (AES-CCM-16-64-128 with partial IV, tag 16)
    let c42 = decode_hex(
        "d08343a1010aa1064261a7581c252a8911d465c125b6764739700f0141ed09192de139e053bd09abca",
    );
    let (tag, untagged) = split_tag(&c42).expect("split c4.2");
    assert_eq!(tag, Some(16));
    let decoded = decode_encrypt0(untagged).expect("decode c4.2");
    let base_iv = decode_hex("89F52F65A1C580930000000000");
    let key_bytes = decode_hex("849B5786457C1491BE3A76DCEA6C4271");
    let key = CoseKey::Symmetric(SymmetricKey {
        common: KeyCommon {
            base_iv: Some(base_iv.as_slice()),
            ..KeyCommon::default()
        },
        k: key_bytes.as_slice(),
    });
    let mut enc_structure = [0u8; 128];
    let mut plaintext = [0u8; 64];
    let plain_len = decrypt_encrypt0_with_cose_key(
        &decoded,
        &key,
        b"",
        None,
        &mut enc_structure,
        &mut plaintext,
    )
    .expect("decrypt c4.2");
    assert_eq!(&plaintext[..plain_len], b"This is the content.");
}
