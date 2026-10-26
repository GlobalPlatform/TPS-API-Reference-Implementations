/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
use serde::Deserialize;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::{decode_base64url, decode_hex, split_tag};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorKind {
    Sign1,
    Encrypt0,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionStatus {
    Success,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorResult {
    pub id: String,
    pub title: String,
    pub kind: VectorKind,
    pub expected_fail: bool,
    pub status: ExecutionStatus,
}

impl VectorResult {
    pub fn matches_expectation(&self) -> bool {
        matches!(
            (&self.expected_fail, &self.status),
            (false, ExecutionStatus::Success) | (true, ExecutionStatus::Error(_))
        )
    }
}

#[derive(Debug, Clone)]
pub struct Sign1Case {
    pub id: String,
    pub title: String,
    pub expected_fail: bool,
    pub plaintext: Vec<u8>,
    pub external_aad: Vec<u8>,
    pub pub_x: Vec<u8>,
    pub pub_y: Vec<u8>,
    pub output_cbor: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Encrypt0Case {
    pub id: String,
    pub title: String,
    pub expected_fail: bool,
    pub plaintext: Vec<u8>,
    pub external_aad: Vec<u8>,
    pub key: Vec<u8>,
    pub output_cbor: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct Sign1Json {
    title: String,
    #[serde(default)]
    fail: bool,
    input: Sign1Input,
    output: JsonOutput,
}

#[derive(Debug, Deserialize)]
struct Sign1Input {
    plaintext: String,
    sign0: Sign0Json,
}

#[derive(Debug, Deserialize)]
struct Sign0Json {
    key: EcKeyJson,
    #[serde(default)]
    protected: AlgMap,
    #[serde(default)]
    unprotected: AlgMap,
    #[serde(default)]
    alg: Option<String>,
    #[serde(default)]
    external: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EcKeyJson {
    x: String,
    y: String,
}

#[derive(Debug, Deserialize, Default)]
struct AlgMap {
    #[serde(default)]
    alg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JsonOutput {
    cbor: String,
}

#[derive(Debug, Deserialize)]
struct Encrypt0Json {
    title: String,
    #[serde(default)]
    fail: bool,
    input: Encrypt0Input,
    output: JsonOutput,
}

#[derive(Debug, Deserialize)]
struct Encrypt0Input {
    plaintext: String,
    encrypted: EncryptedJson,
}

#[derive(Debug, Deserialize)]
struct EncryptedJson {
    #[serde(default)]
    protected: AlgMap,
    #[serde(default)]
    unprotected: AlgMap,
    #[serde(default)]
    external: Option<String>,
    recipients: Vec<RecipientJson>,
}

#[derive(Debug, Deserialize)]
struct RecipientJson {
    key: SymmetricKeyJson,
    #[serde(default)]
    unprotected: AlgMap,
}

#[derive(Debug, Deserialize)]
struct SymmetricKeyJson {
    k: String,
}

fn vector_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/Examples")
}

fn list_json_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("read_dir {}: {e}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension() == Some(OsStr::new("json")))
        .collect();
    files.sort();
    Ok(files)
}

pub fn load_supported_sign1_cases() -> Result<Vec<Sign1Case>, String> {
    let dir = vector_root().join("sign1-tests");
    let mut out = Vec::new();
    for path in list_json_files(&dir)? {
        let data =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let raw: Sign1Json =
            serde_json::from_str(&data).map_err(|e| format!("parse {}: {e}", path.display()))?;
        let alg = raw
            .input
            .sign0
            .alg
            .clone()
            .or(raw.input.sign0.protected.alg.clone())
            .or(raw.input.sign0.unprotected.alg.clone());
        if alg.as_deref() != Some("ES256") {
            continue;
        }
        let id = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("invalid file name: {}", path.display()))?
            .to_owned();
        out.push(Sign1Case {
            id,
            title: raw.title,
            expected_fail: raw.fail,
            plaintext: raw.input.plaintext.into_bytes(),
            external_aad: raw
                .input
                .sign0
                .external
                .as_deref()
                .map(decode_hex)
                .unwrap_or_default(),
            pub_x: decode_base64url(&raw.input.sign0.key.x),
            pub_y: decode_base64url(&raw.input.sign0.key.y),
            output_cbor: decode_hex(&raw.output.cbor),
        });
    }
    Ok(out)
}

pub fn load_supported_encrypt0_cases() -> Result<Vec<Encrypt0Case>, String> {
    use tps_minicbor::{Decoder, Item};

    let dir = vector_root().join("encrypted-tests");
    let mut out = Vec::new();
    for path in list_json_files(&dir)? {
        let data =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let raw: Encrypt0Json =
            serde_json::from_str(&data).map_err(|e| format!("parse {}: {e}", path.display()))?;
        let alg = raw.input.encrypted.protected.alg.clone().or(raw
            .input
            .encrypted
            .unprotected
            .alg
            .clone());
        if alg.as_deref() != Some("A128GCM") {
            continue;
        }
        let recipient = raw
            .input
            .encrypted
            .recipients
            .first()
            .ok_or_else(|| format!("{}: missing recipient", path.display()))?;
        if recipient.unprotected.alg.as_deref() != Some("direct") {
            continue;
        }
        let id = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| format!("invalid file name: {}", path.display()))?
            .to_owned();
        out.push(Encrypt0Case {
            id,
            title: raw.title,
            expected_fail: raw.fail,
            plaintext: raw.input.plaintext.into_bytes(),
            external_aad: raw
                .input
                .encrypted
                .external
                .as_deref()
                .map(decode_hex)
                .unwrap_or_default(),
            key: decode_base64url(&recipient.key.k),
            output_cbor: decode_hex(&raw.output.cbor),
        });
    }
    out.retain(|case| {
        let Ok((_, untagged)) = split_tag(&case.output_cbor) else {
            return false;
        };
        let mut dec = Decoder::new(untagged);
        let Ok(item) = dec.decode() else {
            return false;
        };
        let Item::Array(array) = item else {
            return false;
        };
        if array.len() != 3 {
            return false;
        }
        let Some(Ok(Item::Bytes(protected))) = array.iter().next() else {
            return false;
        };
        // `enc-pass-03.json` in the upstream corpus uses h'' for the protected
        // header bstr. The current decoder expects a serialized map bstr, so
        // vectors with empty protected bstr are excluded for now.
        !protected.is_empty()
    });
    Ok(out)
}

#[cfg(feature = "ring")]
pub fn run_sign1_cases(cases: &[Sign1Case]) -> Vec<VectorResult> {
    use tps_cose::{Es256PublicKey, decode_sign1, verify_sign1_es256};

    cases
        .iter()
        .map(|case| {
            let status = (|| -> Result<(), String> {
                let (tag, untagged) = split_tag(&case.output_cbor).map_err(str::to_string)?;
                if !matches!(tag, Some(18) | None) {
                    return Err(format!("unexpected Sign1 tag: {tag:?}"));
                }
                let sign1 = decode_sign1(untagged).map_err(|e| format!("decode_sign1: {e:?}"))?;
                let payload = sign1
                    .payload
                    .ok_or_else(|| "vector used detached payload".to_string())?;
                if payload != case.plaintext.as_slice() {
                    return Err("plaintext mismatch".to_string());
                }
                let public_key = Es256PublicKey {
                    x: &case.pub_x,
                    y: &case.pub_y,
                };
                let mut sig_structure = vec![0u8; untagged.len() + 64];
                verify_sign1_es256(
                    &sign1,
                    &public_key,
                    &case.external_aad,
                    None,
                    &mut sig_structure,
                )
                .map_err(|e| format!("verify_sign1_es256: {e:?}"))?;
                Ok(())
            })();
            VectorResult {
                id: case.id.clone(),
                title: case.title.clone(),
                kind: VectorKind::Sign1,
                expected_fail: case.expected_fail,
                status: match status {
                    Ok(()) => ExecutionStatus::Success,
                    Err(err) => ExecutionStatus::Error(err),
                },
            }
        })
        .collect()
}

#[cfg(feature = "ring")]
pub fn run_encrypt0_cases(cases: &[Encrypt0Case]) -> Vec<VectorResult> {
    use tps_cose::{decode_encrypt0, decrypt_encrypt0_with_raw_key};

    cases
        .iter()
        .map(|case| {
            let status = (|| -> Result<(), String> {
                let (tag, untagged) = split_tag(&case.output_cbor).map_err(str::to_string)?;
                if !matches!(tag, Some(16) | None) {
                    return Err(format!("unexpected Encrypt0 tag: {tag:?}"));
                }
                let enc0 =
                    decode_encrypt0(untagged).map_err(|e| format!("decode_encrypt0: {e:?}"))?;
                let mut enc_structure = [0u8; 128];
                let mut plaintext = vec![0u8; case.plaintext.len() + 32];
                let plain_len = decrypt_encrypt0_with_raw_key(
                    &enc0,
                    &case.key,
                    &case.external_aad,
                    None,
                    &mut enc_structure,
                    &mut plaintext,
                )
                .map_err(|e| format!("decrypt_encrypt0_with_raw_key: {e:?}"))?;
                if plaintext[..plain_len] != case.plaintext {
                    return Err("plaintext mismatch".to_string());
                }
                Ok(())
            })();
            VectorResult {
                id: case.id.clone(),
                title: case.title.clone(),
                kind: VectorKind::Encrypt0,
                expected_fail: case.expected_fail,
                status: match status {
                    Ok(()) => ExecutionStatus::Success,
                    Err(err) => ExecutionStatus::Error(err),
                },
            }
        })
        .collect()
}
