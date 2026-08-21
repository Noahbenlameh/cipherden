//! Master password -> encryption key derivation.
//!
//! Uses Argon2id exclusively (RustCrypto `argon2` crate). No custom or
//! home-grown KDF is implemented here, per project policy: all cryptographic
//! primitives must come from vetted, maintained libraries.

use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{Result, VaultError};

pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;

/// Argon2id tuning parameters. `standard()` is the production default and
/// should be used for every real vault. Other constructors exist for testing
/// and for future user-configurable "unlock speed" settings, but must never
/// silently become the default for a real vault.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Argon2Params {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Argon2Params {
    /// Production default: 256 MiB memory, 3 iterations, 4-way parallelism.
    /// Deliberately heavier than the OWASP minimum (19 MiB / 2 / 1) because
    /// this key protects every secret in the vault and unlock happens rarely
    /// (once per session), so we can afford ~1-2s on typical hardware.
    pub fn standard() -> Self {
        Self {
            m_cost_kib: 256 * 1024,
            t_cost: 3,
            p_cost: 4,
        }
    }

    /// Fast, weak parameters for tests only. Never use for a real vault.
    pub fn for_testing() -> Self {
        Self {
            m_cost_kib: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }
}

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct VaultKey(pub(crate) [u8; KEY_LEN]);

impl VaultKey {
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }
}

pub fn random_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    salt
}

/// Derive a 32-byte key from a master password, salt, and Argon2id params.
///
/// `password` is zeroized by the caller's responsibility; we take a byte
/// slice and never store or copy it beyond the derivation call.
pub fn derive_key(
    password: &[u8],
    salt: &[u8; SALT_LEN],
    params: Argon2Params,
) -> Result<VaultKey> {
    let argon2_params = Params::new(
        params.m_cost_kib,
        params.t_cost,
        params.p_cost,
        Some(KEY_LEN),
    )
    .map_err(|e| VaultError::Kdf(e.to_string()))?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);

    let mut out = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password, salt, &mut out)
        .map_err(|e| VaultError::Kdf(e.to_string()))?;

    Ok(VaultKey(out))
}
