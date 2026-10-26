/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use std::path::PathBuf;

use tps_cose::{
    Algorithm, CoseKey, Curve, Ec2Y, EdDsaPrivateKey, EdDsaPublicKey, Es256PrivateKey,
    Es256PublicKey, Payload, Sign1Builder, decode_sign1, verify_sign1_eddsa, verify_sign1_es256,
};

use crate::{
    error::CliError,
    io::{decode_hex_arg, read_input, write_output},
};

pub struct SignArgs {
    pub key: PathBuf,
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub aad: Option<String>,
}

pub struct VerifyArgs {
    pub key: PathBuf,
    pub input: Option<PathBuf>,
    pub output: Option<PathBuf>,
    pub aad: Option<String>,
}

pub fn sign(args: SignArgs) -> Result<(), CliError> {
    let key_bytes = std::fs::read(&args.key)?;
    let key = CoseKey::decode(&key_bytes).map_err(CliError::Cose)?;
    let payload = read_input(args.input.as_deref())?;
    let aad = decode_hex_arg(args.aad.as_deref())?;

    let payload_len = payload.len();
    let mut protected_buf = vec![0u8; 256];
    let mut sig_structure_buf = vec![0u8; payload_len + 512];
    let mut signature_buf = vec![0u8; 128];
    let mut out = vec![0u8; payload_len + 1024];

    let n = match &key {
        CoseKey::Ec2(k) if k.curve == Curve::P256 => {
            let d =
                k.d.ok_or_else(|| CliError::msg("key has no private component"))?;
            let (x, y_bytes) = ec2_public_bytes(k)?;
            let priv_key = Es256PrivateKey { d, x, y: y_bytes };
            Sign1Builder::new(&[], &[])
                .algorithm(Algorithm::Es256)
                .external_aad(&aad)
                .payload(Payload::Attached(payload.as_slice()))
                .sign_es256(
                    &priv_key,
                    &mut protected_buf,
                    &mut sig_structure_buf,
                    &mut signature_buf,
                    &mut out,
                )?
        }
        CoseKey::Okp(k) if k.curve == Curve::Ed25519 => {
            let d =
                k.d.ok_or_else(|| CliError::msg("key has no private component"))?;
            let priv_key = EdDsaPrivateKey { d, x: k.x };
            Sign1Builder::new(&[], &[])
                .algorithm(Algorithm::EdDsa)
                .external_aad(&aad)
                .payload(Payload::Attached(payload.as_slice()))
                .sign_eddsa(
                    &priv_key,
                    &mut protected_buf,
                    &mut sig_structure_buf,
                    &mut signature_buf,
                    &mut out,
                )?
        }
        _ => return Err(CliError::msg("unsupported key type for sign1 sign")),
    };

    write_output(args.output.as_deref(), &out[..n])?;
    Ok(())
}

pub fn verify(args: VerifyArgs) -> Result<(), CliError> {
    let key_bytes = std::fs::read(&args.key)?;
    let key = CoseKey::decode(&key_bytes).map_err(CliError::Cose)?;
    let input = read_input(args.input.as_deref())?;
    let aad = decode_hex_arg(args.aad.as_deref())?;

    let sign1 = decode_sign1(&input).map_err(CliError::Cose)?;
    let payload = sign1
        .payload
        .ok_or_else(|| CliError::msg("COSE_Sign1 has detached payload"))?;

    let sig_structure_buf_len = payload.len() + 512;
    let mut sig_structure_buf = vec![0u8; sig_structure_buf_len];

    match &key {
        CoseKey::Ec2(k) if k.curve == Curve::P256 => {
            let (x, y_bytes) = ec2_public_bytes(k)?;
            let pub_key = Es256PublicKey { x, y: y_bytes };
            verify_sign1_es256(&sign1, &pub_key, &aad, None, &mut sig_structure_buf)?;
        }
        CoseKey::Okp(k) if k.curve == Curve::Ed25519 => {
            let pub_key = EdDsaPublicKey { x: k.x };
            verify_sign1_eddsa(&sign1, &pub_key, &aad, None, &mut sig_structure_buf)?;
        }
        _ => return Err(CliError::msg("unsupported key type for sign1 verify")),
    }

    eprintln!("Signature verified.");

    match args.output {
        Some(ref path) => write_output(Some(path.as_path()), payload)?,
        None => println!("{}", hex::encode(payload)),
    }

    Ok(())
}

fn ec2_public_bytes<'a>(k: &'a tps_cose::Ec2Key<'a>) -> Result<(&'a [u8], &'a [u8]), CliError> {
    let y_bytes = match k.y {
        Some(Ec2Y::Bytes(b)) => b,
        _ => return Err(CliError::msg("EC2 key missing uncompressed y-coordinate")),
    };
    Ok((k.x, y_bytes))
}
