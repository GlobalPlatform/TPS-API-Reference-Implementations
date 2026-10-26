/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use std::path::PathBuf;

use ring::rand::{SecureRandom, SystemRandom};
use tps_cose::{
    CoseKey, Encrypt0Builder, EncryptAlgorithm, Payload, decode_encrypt0,
    decrypt_encrypt0_with_cose_key,
};

use crate::{
    error::CliError,
    io::{decode_hex_arg, read_input, write_output},
};

pub struct EncryptArgs {
    pub key: PathBuf,
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub alg: Option<String>,
    pub nonce: Option<String>,
    pub aad: Option<String>,
}

pub struct DecryptArgs {
    pub key: PathBuf,
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub aad: Option<String>,
}

fn parse_alg(s: &str) -> Result<EncryptAlgorithm, CliError> {
    match s {
        "a128gcm" => Ok(EncryptAlgorithm::A128Gcm),
        "a128ccm-16-64-128" => Ok(EncryptAlgorithm::AesCcm16_64_128),
        "a128ccm-16-64-256" => Ok(EncryptAlgorithm::AesCcm16_64_256),
        "a128ccm-64-64-128" => Ok(EncryptAlgorithm::AesCcm64_64_128),
        "a128ccm-64-64-256" => Ok(EncryptAlgorithm::AesCcm64_64_256),
        "a128ccm-16-128-128" => Ok(EncryptAlgorithm::AesCcm16_128_128),
        "a128ccm-16-128-256" => Ok(EncryptAlgorithm::AesCcm16_128_256),
        "a128ccm-64-128-128" => Ok(EncryptAlgorithm::AesCcm64_128_128),
        "a128ccm-64-128-256" => Ok(EncryptAlgorithm::AesCcm64_128_256),
        _ => Err(CliError::msg(format!("unknown algorithm: {s}"))),
    }
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

pub fn encrypt(args: EncryptArgs) -> Result<(), CliError> {
    let key_bytes = std::fs::read(&args.key)?;
    let key = CoseKey::decode(&key_bytes).map_err(CliError::Cose)?;
    let plaintext = read_input(args.input.as_deref())?;
    let aad = decode_hex_arg(args.aad.as_deref())?;
    let alg = args
        .alg
        .as_deref()
        .map(parse_alg)
        .transpose()?
        .unwrap_or(EncryptAlgorithm::A128Gcm);

    let nonce = if let Some(hex) = args.nonce.as_deref() {
        hex::decode(hex)?
    } else {
        let mut buf = vec![0u8; nonce_len(alg)];
        SystemRandom::new()
            .fill(&mut buf)
            .map_err(|_| CliError::msg("failed to generate random nonce"))?;
        buf
    };

    let pt_len = plaintext.len();
    let mut protected_buf = vec![0u8; 256];
    let mut enc_structure_buf = vec![0u8; pt_len + 512];
    let mut ciphertext_buf = vec![0u8; pt_len + 32];
    let mut out = vec![0u8; pt_len + 1024];

    let n = Encrypt0Builder::new(&[], &[])
        .algorithm(alg)
        .payload(Payload::Attached(plaintext.as_slice()))
        .nonce(nonce.as_slice())
        .external_aad(&aad)
        .encrypt_with_cose_key(
            &key,
            &mut protected_buf,
            &mut enc_structure_buf,
            &mut ciphertext_buf,
            &mut out,
        )?;

    write_output(args.output.as_deref(), &out[..n])?;
    Ok(())
}

pub fn decrypt(args: DecryptArgs) -> Result<(), CliError> {
    let key_bytes = std::fs::read(&args.key)?;
    let key = CoseKey::decode(&key_bytes).map_err(CliError::Cose)?;
    let input = read_input(args.input.as_deref())?;
    let aad = decode_hex_arg(args.aad.as_deref())?;

    let encrypt0 = decode_encrypt0(&input).map_err(CliError::Cose)?;
    let ciphertext_len = encrypt0.ciphertext.map(|c| c.len()).unwrap_or(0);

    let mut enc_structure_buf = vec![0u8; ciphertext_len + 512];
    let mut plaintext_buf = vec![0u8; ciphertext_len];

    let n = decrypt_encrypt0_with_cose_key(
        &encrypt0,
        &key,
        &aad,
        None,
        &mut enc_structure_buf,
        &mut plaintext_buf,
    )?;

    match args.output {
        Some(ref path) => write_output(Some(path.as_path()), &plaintext_buf[..n])?,
        None => println!("{}", hex::encode(&plaintext_buf[..n])),
    }

    Ok(())
}
