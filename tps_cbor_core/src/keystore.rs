/***************************************************************************************************
 * Copyright (c) Qualcomm Technologies, Inc. and/or its subsidiaries
 * SPDX-License-Identifier: MIT
 **************************************************************************************************/
//! Flat-file COSE key store.
//!
//! Layout: `<dir>/index.json` holding the metadata, plus one `<dir>/<alias>.cbor` per key holding
//! the raw `CoseKey::encode()` bytes.
//!
//! Ported from `tps_cbor_tui/src/keystore.rs`, which is the better of the two old implementations
//! -- the Tauri backend's `load_key` validated nothing at all, so a `../`-containing alias read
//! arbitrary files. Four things are added here:
//!
//! 1. **Private key files are written `0o600`** on Unix, and the store directory `0o700`. The old
//!    code wrote at the process umask, which on a default Debian install is world-readable.
//! 2. **Loaded key bytes are [`Zeroizing`]**, so a key does not linger on the heap after use.
//! 3. **Overwriting is explicit.** The old Tauri `import_key` silently replaced an existing alias
//!    while its `generate_key` refused one; both now take an [`Overwrite`] and default to refusing.
//! 4. **Metadata is derived from the key, not supplied alongside it.** `import` used to accept a
//!    caller-provided `KeyMetaInput`, so the recorded type and curve could disagree with the
//!    bytes on disk. They are now read back out of the decoded key.
//!
//! Every operation validates its alias through [`validate_alias`] before it can become a
//! filename. That is the single chokepoint; there is no other way to construct a key path.

use std::path::{Path, PathBuf};

use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tps_cose::{CoseKey, EncryptAlgorithm, KeyCommon, SymmetricKey};
use zeroize::Zeroizing;

use crate::cose::keygen::{GenKeyKind, generate_cose_key};
use crate::cose::ops::symmetric_key_len;
use crate::cose::{CurveName, key_curve, key_has_private, key_type_name};

/// File mode for a file holding private key material: owner read/write only.
#[cfg(unix)]
const SECRET_FILE_MODE: u32 = 0o600;

/// Directory mode for the store: owner only, so the alias list is not even enumerable.
#[cfg(unix)]
const STORE_DIR_MODE: u32 = 0o700;

/// Anything that can go wrong in the key store.
#[derive(Debug, Error)]
pub enum KeyStoreError {
    /// A file could not be read, written or removed.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// `index.json` exists but is not the expected JSON.
    #[error("index.json is corrupt: {0}")]
    CorruptIndex(String),

    /// The index lists an alias whose key file is missing or unreadable.
    #[error("key file for alias '{0}' is corrupt or missing")]
    CorruptKey(String),

    /// The alias is not path-safe. See [`validate_alias`].
    #[error(
        "invalid alias '{0}': must be non-empty, at most {max} characters, and contain only \
         ASCII letters, digits, '-' or '_'",
        max = MAX_ALIAS_LEN
    )]
    InvalidAlias(String),

    /// The alias is taken and the caller asked to refuse rather than overwrite.
    #[error("alias '{0}' already exists")]
    AliasExists(String),

    /// No key is stored under that alias.
    #[error("no key found for alias '{0}'")]
    NotFound(String),

    /// The bytes offered are not a decodable COSE_Key, or a key could not be encoded.
    #[error("COSE error: {0:?}")]
    Cose(tps_cose::CoseError),
}

/// The longest alias accepted.
///
/// Not a security boundary -- [`validate_alias`]'s character allowlist is that -- but a filename
/// has a length limit on every filesystem, and failing early with a clear message beats an
/// `ENAMETOOLONG` from the middle of a write.
pub const MAX_ALIAS_LEN: usize = 64;

/// Whether an operation may replace an existing alias.
///
/// There is no default: the old backend had one function that silently overwrote and another that
/// refused, and the inconsistency was invisible at the call site. Making it an argument means the
/// decision is written down wherever a key is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Fail with [`KeyStoreError::AliasExists`] if the alias is taken.
    Refuse,
    /// Replace the existing key and its metadata.
    Replace,
}

/// Metadata describing one stored key, as recorded in `index.json`.
///
/// Richer than the `{alias, size}` the old `list_keys` returned
/// (`tps_cbor_gui/src-tauri/src/main.rs:821-824`), so the UI can say what a key actually *is*
/// without decoding every file to find out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyMeta {
    /// The alias, which is also the file stem.
    pub alias: String,
    /// `"EC2"`, `"OKP"` or `"Symmetric"`.
    pub key_type: String,
    /// The COSE algorithm identifier the key is tagged with, if any.
    pub algorithm: Option<i64>,
    /// The curve name, for EC2 and OKP keys.
    pub curve: Option<String>,
    /// Whether the stored bytes include private key material.
    pub has_private: bool,
    /// When the key was stored, as supplied by the caller.
    ///
    /// A caller-owned string rather than a `SystemTime`, so tests are deterministic and the
    /// format is whatever the frontend already shows.
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct KeyIndex {
    keys: Vec<KeyMeta>,
}

/// A directory of COSE keys.
#[derive(Debug)]
pub struct KeyStore {
    dir: PathBuf,
    index: KeyIndex,
}

type Result<T> = std::result::Result<T, KeyStoreError>;

/// Check that `alias` is safe to use as a filename.
///
/// The rule is an allowlist, not a denylist: non-empty, at most [`MAX_ALIAS_LEN`] characters, and
/// composed only of ASCII alphanumerics, `-` and `_`. That excludes `/`, `\`, `.` and every
/// Unicode lookalike, so no alias can escape the store directory or collide with `index.json`.
///
/// A denylist was tried and failed: the Tauri backend had a looser second copy of this check at
/// `main.rs:800` that allowed any Unicode alphanumeric, which admits characters that normalise to
/// path separators on some filesystems.
pub fn validate_alias(alias: &str) -> Result<()> {
    let ok = !alias.is_empty()
        && alias.len() <= MAX_ALIAS_LEN
        && alias
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(KeyStoreError::InvalidAlias(alias.to_owned()))
    }
}

impl KeyStore {
    /// Open a key store rooted at `dir`, creating the directory and an empty index if absent.
    pub fn open(dir: &Path) -> Result<Self> {
        create_store_dir(dir)?;
        let index_path = dir.join("index.json");
        let index = if index_path.exists() {
            let text = std::fs::read_to_string(&index_path)?;
            serde_json::from_str(&text)
                .map_err(|error| KeyStoreError::CorruptIndex(error.to_string()))?
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

    /// The directory this store is rooted at.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The metadata for every stored key, in insertion order.
    pub fn list(&self) -> &[KeyMeta] {
        &self.index.keys
    }

    /// The metadata for one alias.
    pub fn meta(&self, alias: &str) -> Option<&KeyMeta> {
        self.index.keys.iter().find(|key| key.alias == alias)
    }

    /// Whether `alias` is in use.
    pub fn contains(&self, alias: &str) -> bool {
        self.meta(alias).is_some()
    }

    /// Load the raw COSE_Key bytes for `alias`.
    ///
    /// The result is [`Zeroizing`]: these bytes may contain a private scalar, and the caller
    /// should not have to remember that.
    pub fn load(&self, alias: &str) -> Result<Zeroizing<Vec<u8>>> {
        validate_alias(alias)?;
        if !self.contains(alias) {
            return Err(KeyStoreError::NotFound(alias.to_owned()));
        }
        std::fs::read(self.key_path(alias)?)
            .map(Zeroizing::new)
            .map_err(|_| KeyStoreError::CorruptKey(alias.to_owned()))
    }

    /// Store externally supplied COSE_Key bytes under `alias`.
    ///
    /// The bytes are decoded first, both to reject anything that is not a COSE_Key and to derive
    /// the metadata. Nothing is written if decoding fails.
    pub fn import(
        &mut self,
        alias: &str,
        key_bytes: &[u8],
        created_at: impl Into<String>,
        overwrite: Overwrite,
    ) -> Result<()> {
        validate_alias(alias)?;
        self.check_vacant(alias, overwrite)?;
        let meta = describe(alias, key_bytes, created_at.into())?;
        self.write_key(alias, key_bytes, meta)
    }

    /// Generate a fresh asymmetric keypair and store it under `alias`.
    ///
    /// Returns the metadata recorded, which is what the UI needs to display the new key without
    /// re-reading the store.
    pub fn generate(
        &mut self,
        alias: &str,
        kind: GenKeyKind,
        created_at: impl Into<String>,
        overwrite: Overwrite,
    ) -> Result<KeyMeta> {
        validate_alias(alias)?;
        self.check_vacant(alias, overwrite)?;
        let bytes = generate_cose_key(kind).map_err(KeyStoreError::Cose)?;
        let meta = describe(alias, &bytes, created_at.into())?;
        self.write_key(alias, &bytes, meta.clone())?;
        Ok(meta)
    }

    /// Generate a fresh symmetric key of the right length for `alg` and store it under `alias`.
    ///
    /// The length comes from [`symmetric_key_len`], so a 256-bit algorithm gets a 256-bit key.
    /// The old backend's stub gave every algorithm 16 bytes.
    pub fn generate_symmetric(
        &mut self,
        alias: &str,
        alg: EncryptAlgorithm,
        created_at: impl Into<String>,
        overwrite: Overwrite,
    ) -> Result<KeyMeta> {
        validate_alias(alias)?;
        self.check_vacant(alias, overwrite)?;

        let mut k = Zeroizing::new(vec![0u8; symmetric_key_len(alg)]);
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
        let mut out = Zeroizing::new(vec![0u8; k.len() + 64]);
        let n = key.encode(&mut out).map_err(KeyStoreError::Cose)?;
        out.truncate(n);

        let meta = describe(alias, &out, created_at.into())?;
        self.write_key(alias, &out, meta.clone())?;
        Ok(meta)
    }

    /// Remove a key and its metadata.
    pub fn delete(&mut self, alias: &str) -> Result<()> {
        validate_alias(alias)?;
        let position = self
            .index
            .keys
            .iter()
            .position(|key| key.alias == alias)
            .ok_or_else(|| KeyStoreError::NotFound(alias.to_owned()))?;
        let path = self.key_path(alias)?;
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        self.index.keys.remove(position);
        self.save_index()
    }

    /// The path a key is stored at.
    ///
    /// Private, and the only place an alias becomes a path. It re-validates rather than trusting
    /// that a caller did: this is the function whose misuse the old `load_key` bug consisted of.
    fn key_path(&self, alias: &str) -> Result<PathBuf> {
        validate_alias(alias)?;
        Ok(self.dir.join(format!("{alias}.cbor")))
    }

    fn check_vacant(&self, alias: &str, overwrite: Overwrite) -> Result<()> {
        if overwrite == Overwrite::Refuse && self.contains(alias) {
            return Err(KeyStoreError::AliasExists(alias.to_owned()));
        }
        Ok(())
    }

    /// Write the key file and record `meta`, replacing any existing entry for the same alias.
    fn write_key(&mut self, alias: &str, bytes: &[u8], meta: KeyMeta) -> Result<()> {
        write_secret(&self.key_path(alias)?, bytes)?;
        match self.index.keys.iter().position(|key| key.alias == alias) {
            Some(position) => self.index.keys[position] = meta,
            None => self.index.keys.push(meta),
        }
        self.save_index()
    }

    fn save_index(&self) -> Result<()> {
        let text = serde_json::to_string_pretty(&self.index)
            .map_err(|error| KeyStoreError::CorruptIndex(error.to_string()))?;
        std::fs::write(self.dir.join("index.json"), text)?;
        Ok(())
    }
}

/// Decode `key_bytes` and describe it, so the index cannot disagree with the file.
fn describe(alias: &str, key_bytes: &[u8], created_at: String) -> Result<KeyMeta> {
    let key = CoseKey::decode(key_bytes).map_err(KeyStoreError::Cose)?;
    Ok(KeyMeta {
        alias: alias.to_owned(),
        key_type: key_type_name(&key).to_owned(),
        algorithm: algorithm_of(&key),
        curve: key_curve(&key).map(|curve| CurveName(curve).to_string()),
        has_private: key_has_private(&key),
        created_at,
    })
}

fn algorithm_of(key: &CoseKey<'_>) -> Option<i64> {
    match key {
        CoseKey::Ec2(k) => k.common.algorithm,
        CoseKey::Okp(k) => k.common.algorithm,
        CoseKey::Symmetric(k) => k.common.algorithm,
    }
}

/// Create the store directory, owner-only where the platform supports it.
fn create_store_dir(dir: &Path) -> Result<()> {
    let existed = dir.exists();
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    if !existed {
        // Only on creation: tightening an existing directory the user may have deliberately
        // shared would be surprising, and the key files are protected individually regardless.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(STORE_DIR_MODE))?;
    }
    #[cfg(not(unix))]
    let _ = existed;
    Ok(())
}

/// Write `bytes` to `path` with owner-only permissions where the platform supports it.
///
/// The mode is applied at creation through `OpenOptions`, not afterwards, so there is no window in
/// which the file exists with private key material at the process umask. An existing file is
/// truncated and its mode reset, which matters for [`Overwrite::Replace`].
fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(SECRET_FILE_MODE);
    }
    let mut file = options.open(path)?;

    #[cfg(unix)]
    {
        // `OpenOptions::mode` applies only when the file is created, so an alias being replaced
        // would keep whatever mode it already had.
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(SECRET_FILE_MODE))?;
    }

    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory unique to `name`.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tps_cbor_core_keystore_{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn symmetric_bytes(len: usize) -> Vec<u8> {
        let k = vec![7u8; len];
        let key = CoseKey::Symmetric(SymmetricKey::new(&k));
        let mut buf = [0u8; 128];
        let n = key.encode(&mut buf).expect("encode");
        buf[..n].to_vec()
    }

    #[test]
    fn open_creates_the_directory_and_an_empty_index() {
        let dir = temp_dir("open_creates");
        let store = KeyStore::open(&dir).expect("open");
        assert!(store.list().is_empty());
        assert!(dir.join("index.json").exists());
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn generate_list_load_delete_round_trip() {
        let dir = temp_dir("round_trip");
        let mut store = KeyStore::open(&dir).expect("open");
        let meta = store
            .generate(
                "mykey",
                GenKeyKind::Ec2P256,
                "2026-10-28",
                Overwrite::Refuse,
            )
            .expect("generate");

        assert_eq!(meta.key_type, "EC2");
        assert_eq!(meta.curve.as_deref(), Some("P-256"));
        assert_eq!(meta.algorithm, Some(-7));
        assert!(meta.has_private);
        assert_eq!(store.list(), std::slice::from_ref(&meta));

        let bytes = store.load("mykey").expect("load");
        assert!(matches!(
            CoseKey::decode(&bytes).expect("decode"),
            CoseKey::Ec2(_)
        ));

        store.delete("mykey").expect("delete");
        assert!(store.list().is_empty());
        assert!(matches!(
            store.load("mykey"),
            Err(KeyStoreError::NotFound(_))
        ));
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    /// The security fix: an alias that would escape the directory is rejected on every path in.
    #[test]
    fn a_traversing_alias_is_rejected_by_every_operation() {
        let dir = temp_dir("traversal");
        let mut store = KeyStore::open(&dir).expect("open");
        for alias in ["../escape", "..", "a/b", "a\\b", "", "has space", "dot.ted"] {
            assert!(
                matches!(store.load(alias), Err(KeyStoreError::InvalidAlias(_))),
                "load accepted {alias:?}"
            );
            assert!(
                matches!(store.delete(alias), Err(KeyStoreError::InvalidAlias(_))),
                "delete accepted {alias:?}"
            );
            assert!(
                matches!(
                    store.import(alias, &symmetric_bytes(16), "now", Overwrite::Refuse),
                    Err(KeyStoreError::InvalidAlias(_))
                ),
                "import accepted {alias:?}"
            );
            assert!(
                matches!(
                    store.generate(alias, GenKeyKind::Ec2P256, "now", Overwrite::Refuse),
                    Err(KeyStoreError::InvalidAlias(_))
                ),
                "generate accepted {alias:?}"
            );
            assert!(
                matches!(
                    store.generate_symmetric(
                        alias,
                        EncryptAlgorithm::A128Gcm,
                        "now",
                        Overwrite::Refuse
                    ),
                    Err(KeyStoreError::InvalidAlias(_))
                ),
                "generate_symmetric accepted {alias:?}"
            );
        }
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    /// The looser second copy of the check in the old backend allowed Unicode alphanumerics.
    #[test]
    fn a_unicode_alias_is_rejected() {
        let dir = temp_dir("unicode");
        let mut store = KeyStore::open(&dir).expect("open");
        for alias in ["ключ", "キー", "ké"] {
            assert!(
                matches!(
                    store.generate(alias, GenKeyKind::Ec2P256, "now", Overwrite::Refuse),
                    Err(KeyStoreError::InvalidAlias(_))
                ),
                "accepted {alias:?}"
            );
        }
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn an_overlong_alias_is_rejected() {
        let dir = temp_dir("overlong");
        let mut store = KeyStore::open(&dir).expect("open");
        let alias = "a".repeat(MAX_ALIAS_LEN + 1);
        assert!(matches!(
            store.generate(&alias, GenKeyKind::Ec2P256, "now", Overwrite::Refuse),
            Err(KeyStoreError::InvalidAlias(_))
        ));
        let longest = "a".repeat(MAX_ALIAS_LEN);
        store
            .generate(&longest, GenKeyKind::Ec2P256, "now", Overwrite::Refuse)
            .expect("the limit itself is allowed");
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn refuse_is_refused_and_replace_replaces() {
        let dir = temp_dir("overwrite");
        let mut store = KeyStore::open(&dir).expect("open");
        store
            .generate("k1", GenKeyKind::Ec2P256, "first", Overwrite::Refuse)
            .expect("generate");
        let first = store.load("k1").expect("load").to_vec();

        let error = store
            .generate("k1", GenKeyKind::Ec2P256, "second", Overwrite::Refuse)
            .expect_err("should refuse");
        assert!(matches!(error, KeyStoreError::AliasExists(_)));
        assert_eq!(store.load("k1").expect("load").to_vec(), first);
        assert_eq!(store.meta("k1").expect("meta").created_at, "first");

        store
            .generate("k1", GenKeyKind::OkpEd25519, "third", Overwrite::Replace)
            .expect("should replace");
        assert_ne!(store.load("k1").expect("load").to_vec(), first);
        assert_eq!(
            store.list().len(),
            1,
            "replace must not duplicate the entry"
        );
        let meta = store.meta("k1").expect("meta");
        assert_eq!(meta.key_type, "OKP");
        assert_eq!(meta.created_at, "third");
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn metadata_is_derived_from_the_stored_bytes() {
        let dir = temp_dir("derived_meta");
        let mut store = KeyStore::open(&dir).expect("open");
        store
            .import("imported", &symmetric_bytes(32), "now", Overwrite::Refuse)
            .expect("import");
        let meta = store.meta("imported").expect("meta");
        assert_eq!(meta.key_type, "Symmetric");
        assert_eq!(meta.curve, None);
        assert!(meta.has_private);
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_256_bit_algorithm_gets_a_32_byte_key() {
        let dir = temp_dir("key_len");
        let mut store = KeyStore::open(&dir).expect("open");
        store
            .generate_symmetric(
                "wide",
                EncryptAlgorithm::AesCcm16_64_256,
                "now",
                Overwrite::Refuse,
            )
            .expect("generate");
        let bytes = store.load("wide").expect("load");
        let CoseKey::Symmetric(key) = CoseKey::decode(&bytes).expect("decode") else {
            panic!("not symmetric")
        };
        assert_eq!(key.k.len(), 32);
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn import_rejects_bytes_that_are_not_a_cose_key() {
        let dir = temp_dir("import_invalid");
        let mut store = KeyStore::open(&dir).expect("open");
        let error = store
            .import("bad", b"not a cose key", "now", Overwrite::Refuse)
            .expect_err("should reject");
        assert!(matches!(error, KeyStoreError::Cose(_)));
        // Nothing was written: the decode happens before any file is touched.
        assert!(store.list().is_empty());
        assert!(!dir.join("bad.cbor").exists());
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_corrupt_index_is_reported_rather_than_silently_emptied() {
        let dir = temp_dir("corrupt_index");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("index.json"), "not json").expect("write");
        let error = KeyStore::open(&dir).expect_err("should fail");
        assert!(matches!(error, KeyStoreError::CorruptIndex(_)));
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn a_missing_key_file_is_reported_as_corrupt() {
        let dir = temp_dir("missing_file");
        let mut store = KeyStore::open(&dir).expect("open");
        store
            .generate_symmetric("k1", EncryptAlgorithm::A128Gcm, "now", Overwrite::Refuse)
            .expect("generate");
        std::fs::remove_file(dir.join("k1.cbor")).expect("remove");
        assert!(matches!(
            store.load("k1"),
            Err(KeyStoreError::CorruptKey(_))
        ));
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn the_index_survives_a_reopen() {
        let dir = temp_dir("reopen");
        {
            let mut store = KeyStore::open(&dir).expect("open");
            store
                .generate("k1", GenKeyKind::OkpEd25519, "now", Overwrite::Refuse)
                .expect("generate");
        }
        let store = KeyStore::open(&dir).expect("reopen");
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.meta("k1").expect("meta").key_type, "OKP");
        store.load("k1").expect("load after reopen");
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[cfg(unix)]
    #[test]
    fn private_key_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("permissions");
        let mut store = KeyStore::open(&dir).expect("open");
        store
            .generate("k1", GenKeyKind::Ec2P256, "now", Overwrite::Refuse)
            .expect("generate");

        let mode = std::fs::metadata(dir.join("k1.cbor"))
            .expect("stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, SECRET_FILE_MODE, "key file mode was {mode:o}");

        let dir_mode = std::fs::metadata(&dir).expect("stat").permissions().mode() & 0o777;
        assert_eq!(dir_mode, STORE_DIR_MODE, "store dir mode was {dir_mode:o}");
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    /// `OpenOptions::mode` applies only at creation, so replacing a key must reset the mode too.
    #[cfg(unix)]
    #[test]
    fn replacing_a_key_tightens_a_loose_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp_dir("retighten");
        let mut store = KeyStore::open(&dir).expect("open");
        store
            .generate("k1", GenKeyKind::Ec2P256, "now", Overwrite::Refuse)
            .expect("generate");

        let path = dir.join("k1.cbor");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("loosen");
        store
            .generate("k1", GenKeyKind::Ec2P256, "now", Overwrite::Replace)
            .expect("replace");

        let mode = std::fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode, SECRET_FILE_MODE, "mode stayed {mode:o}");
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[test]
    fn validate_alias_accepts_the_allowlist() {
        for alias in ["a", "A1", "my-key", "my_key", "0"] {
            validate_alias(alias).unwrap_or_else(|e| panic!("{alias:?} rejected: {e}"));
        }
    }

    #[test]
    fn the_index_file_name_cannot_be_claimed_as_an_alias() {
        // `index` is a legal alias, but it becomes `index.cbor`, not `index.json`, so the
        // metadata file is unreachable through the alias namespace.
        let dir = temp_dir("index_alias");
        let mut store = KeyStore::open(&dir).expect("open");
        store
            .generate("index", GenKeyKind::Ec2P256, "now", Overwrite::Refuse)
            .expect("generate");
        assert!(dir.join("index.cbor").exists());
        assert_eq!(store.list().len(), 1, "index.json was clobbered");
        std::fs::remove_dir_all(&dir).expect("clean up");
    }
}
