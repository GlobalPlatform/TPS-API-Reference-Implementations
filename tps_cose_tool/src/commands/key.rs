/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use std::path::PathBuf;

use tps_cose::{CoseKey, Curve, KeyType};

use crate::error::CliError;

pub struct InfoArgs {
    pub key: PathBuf,
}

pub fn info(args: InfoArgs) -> Result<(), CliError> {
    let key_bytes = std::fs::read(&args.key)?;
    let key = CoseKey::decode(&key_bytes).map_err(CliError::Cose)?;

    match key {
        CoseKey::Ec2(k) => {
            println!("Key type:  EC2 ({})", KeyType::Ec2.to_i64());
            println!("Curve:     {}", curve_name(k.curve));
            print_common(k.common.key_id, k.common.algorithm);
            println!("Has private key (d): {}", k.d.is_some());
        }
        CoseKey::Okp(k) => {
            println!("Key type:  OKP ({})", KeyType::Okp.to_i64());
            println!("Curve:     {}", curve_name(k.curve));
            print_common(k.common.key_id, k.common.algorithm);
            println!("Has private key (d): {}", k.d.is_some());
        }
        CoseKey::Symmetric(k) => {
            println!("Key type:  Symmetric ({})", KeyType::Symmetric.to_i64());
            println!("Key length: {} bytes", k.k.len());
            print_common(k.common.key_id, k.common.algorithm);
        }
    }
    Ok(())
}

fn curve_name(c: Curve) -> String {
    match c {
        Curve::P256 => "P-256".to_owned(),
        Curve::P384 => "P-384".to_owned(),
        Curve::P521 => "P-521".to_owned(),
        Curve::X25519 => "X25519".to_owned(),
        Curve::X448 => "X448".to_owned(),
        Curve::Ed25519 => "Ed25519".to_owned(),
        Curve::Ed448 => "Ed448".to_owned(),
        Curve::Other(v) => format!("unknown ({v})"),
    }
}

fn print_common(key_id: Option<&[u8]>, algorithm: Option<i64>) {
    if let Some(kid) = key_id {
        println!("Key ID:    {}", hex::encode(kid));
    } else {
        println!("Key ID:    (none)");
    }
    if let Some(alg) = algorithm {
        println!("Algorithm: {alg}");
    }
}
