/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Flat-file COSE key store.
//!
//! Layout: `<dir>/index.json` (JSON metadata) plus one `<dir>/<alias>.cbor`
//! file per key, holding the raw `CoseKey::encode()` bytes. Aliases are
//! validated against a path-safe allowlist before ever becoming a filename,
//! so this can't be used to escape `dir`.

use std::path::{Path, PathBuf};

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tps_cose::{CoseKey, EncryptAlgorithm, KeyCommon, SymmetricKey};

#[derive(Debug, Error)]
pub enum KeyStoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("index.json is corrupt: {0}")]
    CorruptIndex(String),

    #[error("key file for alias '{0}' is corrupt or missing")]
    CorruptKey(String),

    #[error("invalid alias '{0}': must be non-empty and contain only ASCII letters, digits, '-', or '_'")]
    InvalidAlias(String),

    #[error("alias '{0}' already exists")]
    AliasExists(String),

    #[error("no key found for alias '{0}'")]
    NotFound(String),

    #[error("COSE error: {0:?}")]
    Cose(tps_cose::CoseError),
}

/// Metadata describing one stored key, as recorded in `index.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMeta {
    pub alias: String,
    pub key_type: String,
    pub algorithm: Option<i64>,
    pub curve: Option<String>,
    pub has_private: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KeyIndex {
    keys: Vec<KeyMeta>,
}

/// Inputs needed to record a key's metadata; `created_at` is supplied by the
/// caller (this crate cannot call `SystemTime::now()`/`Instant` restrictions
/// don't apply here at runtime, but callers own the timestamp source so
/// tests stay deterministic).
pub struct KeyMetaInput {
    pub algorithm: Option<i64>,
    pub curve: Option<String>,
    pub has_private: bool,
    pub created_at: String,
}

#[derive(Debug)]
pub struct KeyStore {
    dir: PathBuf,
    index: KeyIndex,
}

fn validate_alias(alias: &str) -> Result<(), KeyStoreError> {
    let ok = !alias.is_empty()
        && alias
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(KeyStoreError::InvalidAlias(alias.to_owned()))
    }
}

fn key_type_name(key: &CoseKey<'_>) -> &'static str {
    match key {
        CoseKey::Ec2(_) => "EC2",
        CoseKey::Okp(_) => "OKP",
        CoseKey::Symmetric(_) => "Symmetric",
    }
}

impl KeyStore {
    /// Open (creating if absent) a key store rooted at `dir`.
    pub fn open(dir: &Path) -> Result<Self, KeyStoreError> {
        std::fs::create_dir_all(dir)?;
        let index_path = dir.join("index.json");
        let index = if index_path.exists() {
            let text = std::fs::read_to_string(&index_path)?;
            serde_json::from_str(&text).map_err(|e| KeyStoreError::CorruptIndex(e.to_string()))?
        } else {
            KeyIndex::default()
        };
        let store = Self {
            dir: dir.to_path_buf(),
            index,
        };
        if !index_path.exists() {
            store.save_index()?;
        }
        Ok(store)
    }

    fn save_index(&self) -> Result<(), KeyStoreError> {
        let text = serde_json::to_string_pretty(&self.index)
            .map_err(|e| KeyStoreError::CorruptIndex(e.to_string()))?;
        std::fs::write(self.dir.join("index.json"), text)?;
        Ok(())
    }

    fn key_path(&self, alias: &str) -> PathBuf {
        self.dir.join(format!("{alias}.cbor"))
    }

    pub fn list(&self) -> &[KeyMeta] {
        &self.index.keys
    }

    /// The directory this key store is rooted at.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Load the raw COSE_Key bytes for `alias`.
    pub fn load(&self, alias: &str) -> Result<Vec<u8>, KeyStoreError> {
        validate_alias(alias)?;
        if !self.index.keys.iter().any(|k| k.alias == alias) {
            return Err(KeyStoreError::NotFound(alias.to_owned()));
        }
        std::fs::read(self.key_path(alias)).map_err(|_| KeyStoreError::CorruptKey(alias.to_owned()))
    }

    /// Import an externally-generated key, recording its metadata.
    pub fn import(
        &mut self,
        alias: &str,
        key_bytes: &[u8],
        meta: KeyMetaInput,
    ) -> Result<(), KeyStoreError> {
        validate_alias(alias)?;
        if self.index.keys.iter().any(|k| k.alias == alias) {
            return Err(KeyStoreError::AliasExists(alias.to_owned()));
        }
        let key = CoseKey::decode(key_bytes).map_err(KeyStoreError::Cose)?;
        let key_type = key_type_name(&key).to_owned();

        std::fs::write(self.key_path(alias), key_bytes)?;
        self.index.keys.push(KeyMeta {
            alias: alias.to_owned(),
            key_type,
            algorithm: meta.algorithm,
            curve: meta.curve,
            has_private: meta.has_private,
            created_at: meta.created_at,
        });
        self.save_index()?;
        Ok(())
    }

    /// Remove a key and its metadata.
    pub fn delete(&mut self, alias: &str) -> Result<(), KeyStoreError> {
        validate_alias(alias)?;
        let idx = self
            .index
            .keys
            .iter()
            .position(|k| k.alias == alias)
            .ok_or_else(|| KeyStoreError::NotFound(alias.to_owned()))?;
        let path = self.key_path(alias);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        self.index.keys.remove(idx);
        self.save_index()?;
        Ok(())
    }

    /// Generate and store a fresh symmetric key for `alg`.
    pub fn generate_symmetric(
        &mut self,
        alias: &str,
        alg: EncryptAlgorithm,
        created_at: String,
    ) -> Result<(), KeyStoreError> {
        validate_alias(alias)?;
        if self.index.keys.iter().any(|k| k.alias == alias) {
            return Err(KeyStoreError::AliasExists(alias.to_owned()));
        }

        let mut k = vec![0u8; symmetric_key_len(alg)];
        SystemRandom::new()
            .fill(&mut k)
            .map_err(|_| KeyStoreError::Cose(tps_cose::CoseError::InvalidKey))?;

        let key = CoseKey::Symmetric(SymmetricKey {
            common: KeyCommon {
                key_id: None,
                algorithm: Some(alg.to_i64()),
                base_iv: None,
            },
            k: &k,
        });
        let mut out = vec![0u8; k.len() + 64];
        let n = key.encode(&mut out).map_err(KeyStoreError::Cose)?;

        std::fs::write(self.key_path(alias), &out[..n])?;
        self.index.keys.push(KeyMeta {
            alias: alias.to_owned(),
            key_type: "Symmetric".to_owned(),
            algorithm: Some(alg.to_i64()),
            curve: None,
            has_private: false,
            created_at,
        });
        self.save_index()?;
        Ok(())
    }
}

fn symmetric_key_len(alg: EncryptAlgorithm) -> usize {
    match alg {
        EncryptAlgorithm::A128Gcm
        | EncryptAlgorithm::AesCcm16_64_128
        | EncryptAlgorithm::AesCcm64_64_128
        | EncryptAlgorithm::AesCcm16_128_128
        | EncryptAlgorithm::AesCcm64_128_128 => 16,
        EncryptAlgorithm::AesCcm16_64_256
        | EncryptAlgorithm::AesCcm64_64_256
        | EncryptAlgorithm::AesCcm16_128_256
        | EncryptAlgorithm::AesCcm64_128_256 => 32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tps_cbor_tui_keystore_test_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn open_creates_dir_and_empty_index() {
        let dir = temp_dir("open_creates");
        let store = KeyStore::open(&dir).unwrap();
        assert!(store.list().is_empty());
        assert!(dir.join("index.json").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn generate_list_load_delete_round_trip() {
        let dir = temp_dir("round_trip");
        let mut store = KeyStore::open(&dir).unwrap();
        store
            .generate_symmetric("mykey", EncryptAlgorithm::A128Gcm, "2026-07-09".to_owned())
            .unwrap();

        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].alias, "mykey");

        let bytes = store.load("mykey").unwrap();
        let key = CoseKey::decode(&bytes).unwrap();
        assert!(matches!(key, CoseKey::Symmetric(_)));

        store.delete("mykey").unwrap();
        assert!(store.list().is_empty());
        assert!(matches!(store.load("mykey"), Err(KeyStoreError::NotFound(_))));

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn duplicate_alias_is_rejected() {
        let dir = temp_dir("dup_alias");
        let mut store = KeyStore::open(&dir).unwrap();
        store
            .generate_symmetric("k1", EncryptAlgorithm::A128Gcm, "2026-07-09".to_owned())
            .unwrap();
        let err = store
            .generate_symmetric("k1", EncryptAlgorithm::A128Gcm, "2026-07-09".to_owned())
            .unwrap_err();
        assert!(matches!(err, KeyStoreError::AliasExists(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn invalid_alias_is_rejected() {
        let dir = temp_dir("invalid_alias");
        let mut store = KeyStore::open(&dir).unwrap();
        let err = store
            .generate_symmetric("../escape", EncryptAlgorithm::A128Gcm, "2026-07-09".to_owned())
            .unwrap_err();
        assert!(matches!(err, KeyStoreError::InvalidAlias(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn corrupt_index_is_reported() {
        let dir = temp_dir("corrupt_index");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.json"), "not json").unwrap();
        let err = KeyStore::open(&dir).unwrap_err();
        assert!(matches!(err, KeyStoreError::CorruptIndex(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_key_file_is_reported_as_corrupt() {
        let dir = temp_dir("missing_key_file");
        let mut store = KeyStore::open(&dir).unwrap();
        store
            .generate_symmetric("k1", EncryptAlgorithm::A128Gcm, "2026-07-09".to_owned())
            .unwrap();
        std::fs::remove_file(dir.join("k1.cbor")).unwrap();
        let err = store.load("k1").unwrap_err();
        assert!(matches!(err, KeyStoreError::CorruptKey(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn import_decodes_and_records_metadata() {
        let dir = temp_dir("import");
        let mut store = KeyStore::open(&dir).unwrap();

        let k = [7u8; 16];
        let key = CoseKey::Symmetric(SymmetricKey::new(&k));
        let mut buf = [0u8; 64];
        let n = key.encode(&mut buf).unwrap();

        store
            .import(
                "imported",
                &buf[..n],
                KeyMetaInput {
                    algorithm: Some(EncryptAlgorithm::A128Gcm.to_i64()),
                    curve: None,
                    has_private: false,
                    created_at: "2026-07-09".to_owned(),
                },
            )
            .unwrap();

        assert_eq!(store.list().len(), 1);
        let loaded = store.load("imported").unwrap();
        assert_eq!(&loaded, &buf[..n]);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn import_rejects_invalid_cose_key_bytes() {
        let dir = temp_dir("import_invalid");
        let mut store = KeyStore::open(&dir).unwrap();
        let err = store
            .import(
                "bad",
                b"not a cose key",
                KeyMetaInput {
                    algorithm: None,
                    curve: None,
                    has_private: false,
                    created_at: "2026-07-09".to_owned(),
                },
            )
            .unwrap_err();
        assert!(matches!(err, KeyStoreError::Cose(_)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
