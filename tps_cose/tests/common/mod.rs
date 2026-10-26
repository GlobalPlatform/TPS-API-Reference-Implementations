/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use base64::Engine;
use serde::Deserialize;

#[allow(dead_code)]
pub mod ietf_examples;

#[derive(Debug, Default, Deserialize)]
pub struct HeaderMap {
    pub alg: Option<String>,
    pub kid: Option<String>,
    pub ctyp: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct Sign1Vector {
    pub title: String,
    pub input: Sign1Input,
    pub intermediates: Option<Intermediates>,
    pub output: Output,
    #[serde(default)]
    pub fail: bool,
}

#[derive(Debug, Deserialize)]
pub struct Sign1Input {
    pub plaintext: String,
    pub sign0: Sign0Input,
}

#[cfg_attr(not(feature = "ring"), allow(dead_code))]
#[derive(Debug, Deserialize)]
pub struct Sign0Input {
    pub key: EcKey,
    #[serde(default)]
    pub protected: HeaderMap,
    #[serde(default)]
    pub unprotected: HeaderMap,
    pub external: Option<String>,
}

#[cfg_attr(not(feature = "ring"), allow(dead_code))]
#[derive(Debug, Deserialize)]
pub struct EcKey {
    pub x: String,
    pub y: String,
    pub d: String,
}

#[derive(Debug, Deserialize)]
pub struct Intermediates {
    #[serde(rename = "ToBeSign_hex")]
    pub to_be_sign_hex: String,
}

#[derive(Debug, Deserialize)]
pub struct Output {
    pub cbor: String,
}

#[derive(Debug, Deserialize)]
pub struct SignVector {
    pub title: String,
    pub input: SignInput,
    pub intermediates: SignIntermediates,
    pub output: Output,
}

#[derive(Debug, Deserialize)]
pub struct SignInput {
    pub plaintext: String,
    pub sign: SignInputData,
}

#[derive(Debug, Deserialize)]
pub struct SignInputData {
    #[serde(default)]
    pub protected: HeaderMap,
    pub signers: Vec<SignerInput>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SignerInput {
    #[serde(default)]
    pub protected: HeaderMap,
    #[serde(default)]
    pub unprotected: HeaderMap,
}

#[derive(Debug, Deserialize)]
pub struct SignIntermediates {
    pub signers: Vec<SignerIntermediates>,
}

#[derive(Debug, Deserialize)]
pub struct SignerIntermediates {
    #[serde(rename = "ToBeSign_hex")]
    pub to_be_sign_hex: String,
}

pub fn load_sign1_vector(name: &str) -> Sign1Vector {
    let path = vector_path(name);
    let data = std::fs::read_to_string(&path).expect("read sign1 vector");
    serde_json::from_str(&data).expect("parse sign1 vector")
}

pub fn load_sign_vector(name: &str) -> SignVector {
    let path = vector_path(name);
    let data = std::fs::read_to_string(&path).expect("read sign vector");
    serde_json::from_str(&data).expect("parse sign vector")
}

pub fn decode_hex(input: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() / 2);
    let mut iter = input.as_bytes().iter().copied();
    while let (Some(high), Some(low)) = (iter.next(), iter.next()) {
        let hi = hex_value(high).expect("hex high");
        let lo = hex_value(low).expect("hex low");
        out.push((hi << 4) | lo);
    }
    out
}

#[cfg_attr(not(feature = "ring"), allow(dead_code))]
pub fn decode_base64url(input: &str) -> Vec<u8> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(input)
        .expect("decode base64url")
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn split_tag(data: &[u8]) -> Result<(Option<u64>, &[u8]), &'static str> {
    if data.is_empty() {
        return Err("empty CBOR data");
    }
    let initial = data[0];
    if initial >> 5 != 6 {
        return Ok((None, data));
    }
    let additional = initial & 0x1f;
    match additional {
        0..=23 => Ok((Some(additional as u64), &data[1..])),
        24 => {
            if data.len() < 2 {
                return Err("truncated tag");
            }
            Ok((Some(data[1] as u64), &data[2..]))
        }
        25 => {
            if data.len() < 3 {
                return Err("truncated tag");
            }
            let tag = u16::from_be_bytes([data[1], data[2]]) as u64;
            Ok((Some(tag), &data[3..]))
        }
        _ => Err("unsupported tag encoding"),
    }
}

pub fn encode_tag(tag: u64, content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 3);
    if tag <= 23 {
        out.push(0b1100_0000 | tag as u8);
    } else if tag <= 0xff {
        out.push(0b1101_1000);
        out.push(tag as u8);
    } else if tag <= 0xffff {
        out.push(0b1101_1001);
        out.extend_from_slice(&(tag as u16).to_be_bytes());
    } else {
        panic!("tag too large for test helper");
    }
    out.extend_from_slice(content);
    out
}

fn vector_path(name: &str) -> String {
    let root = env!("CARGO_MANIFEST_DIR");
    format!("{root}/tests/vectors/{name}")
}
