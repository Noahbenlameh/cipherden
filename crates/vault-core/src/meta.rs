//! Unencrypted sidecar metadata stored next to the vault's SQLCipher file.
//!
//! Nothing in here is secret: a KDF salt only needs to be unique and random,
//! not hidden, and the Argon2id parameters must be readable before we have
//! the key to derive from the password. This mirrors how KeePass and
//! VeraCrypt store KDF parameters in an unencrypted header.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Result, VaultError};
use crate::kdf::{Argon2Params, SALT_LEN};

const META_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct VaultMeta {
    pub version: u32,
    pub salt: String, // base64
    pub argon2: Argon2Params,
    pub created_at: String, // RFC3339
}

impl VaultMeta {
    pub fn new(salt: &[u8; SALT_LEN], argon2: Argon2Params) -> Self {
        Self {
            version: META_VERSION,
            salt: base64_encode(salt),
            argon2,
            created_at: crate::now_rfc3339(),
        }
    }

    pub fn salt_bytes(&self) -> Result<[u8; SALT_LEN]> {
        let bytes = base64_decode(&self.salt)
            .map_err(|e| VaultError::InvalidMeta(format!("bad salt encoding: {e}")))?;
        bytes
            .try_into()
            .map_err(|_| VaultError::InvalidMeta("salt has wrong length".into()))
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = fs::read_to_string(path)?;
        let meta: Self = serde_json::from_str(&data)?;
        Ok(meta)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        fs::write(path, data)?;
        Ok(())
    }
}

pub fn meta_path_for(db_path: &Path) -> std::path::PathBuf {
    let mut p = db_path.to_path_buf();
    let file_name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    p.set_file_name(format!("{file_name}.meta.json"));
    p
}

// Minimal base64 (standard, no padding stripped) without pulling in a whole
// extra crate just for this; used for salts and wrapped keys, all small.
pub(crate) fn base64_encode(bytes: &[u8]) -> String {
    use base64_impl::Engine;
    base64_impl::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn base64_decode(s: &str) -> std::result::Result<Vec<u8>, String> {
    use base64_impl::Engine;
    base64_impl::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| e.to_string())
}

mod base64_impl {
    pub use base64::*;
}
