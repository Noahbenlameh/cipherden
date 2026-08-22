//! Master password -> encryption key derivation.
//!
//! Uses Argon2id exclusively (RustCrypto `argon2` crate). No custom or
//! home-grown KDF is implemented here, per project policy: all cryptographic
//! primitives must come from vetted, maintained libraries.

use std::alloc::{alloc, dealloc, Layout};
use std::ptr::NonNull;

use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

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

/// A 32-byte key, held in a dedicated, page-locked (`mlock`/`VirtualLock`)
/// heap allocation so it's never paged out to disk (swap), and zeroed on
/// drop. Each `VaultKey` gets its own OS page (rather than sharing a heap
/// page with unrelated allocations) so that one key's lock/unlock lifecycle
/// can never affect another's.
///
/// Locking is best-effort: some environments (containers, restrictive
/// `RLIMIT_MEMLOCK`) refuse it for an unprivileged process. We still
/// function without it — the key is still zeroized on drop and out of the
/// normal allocator's reuse pool during its lifetime either way, this just
/// hardens the "never touches swap while live" property specifically.
pub struct VaultKey {
    ptr: NonNull<u8>,
    layout: Layout,
    _lock: Option<region::LockGuard>,
}

// SAFETY: `VaultKey` owns its allocation exclusively (nothing else ever
// gets a pointer into it) and exposes only shared (`&`) access to the
// bytes, so sending it to / sharing it across threads is sound — exactly
// the same reasoning that makes `Box<[u8; N]>` Send+Sync, plus the same
// reasoning `region::LockGuard` itself is already Send+Sync for.
unsafe impl Send for VaultKey {}
unsafe impl Sync for VaultKey {}

impl VaultKey {
    fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        let page_size = region::page::size().max(KEY_LEN);
        let layout = Layout::from_size_align(page_size, page_size)
            .expect("OS page size is always a valid non-zero power-of-two alignment");

        // SAFETY: `layout` has non-zero size.
        let raw = unsafe { alloc(layout) };
        let ptr = NonNull::new(raw).expect("single-page allocation failure is unrecoverable");

        // SAFETY: `ptr` points to a fresh `page_size`-byte allocation and
        // `KEY_LEN <= page_size`, so writing `KEY_LEN` bytes is in bounds
        // and doesn't overlap `bytes`.
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.as_ptr(), KEY_LEN) };

        let lock = region::lock(ptr.as_ptr(), page_size).ok();

        Self {
            ptr,
            layout,
            _lock: lock,
        }
    }

    /// Generate a fresh random 32-byte key directly (not derived from a
    /// password) — used as a Shell's actual data-encryption key, which is
    /// then wrapped under one or more password-derived key-encryption keys
    /// (see `keyslots.rs`) rather than being an Argon2id output itself.
    pub fn random() -> Self {
        let mut bytes = [0u8; KEY_LEN];
        rand::thread_rng().fill_bytes(&mut bytes);
        let key = Self::from_bytes(bytes);
        bytes.zeroize();
        key
    }

    pub(crate) fn from_raw(bytes: [u8; KEY_LEN]) -> Self {
        Self::from_bytes(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        // SAFETY: the first `KEY_LEN` bytes of this allocation were
        // initialized by `from_bytes` and never written to since.
        unsafe { &*self.ptr.as_ptr().cast::<[u8; KEY_LEN]>() }
    }
}

impl Drop for VaultKey {
    fn drop(&mut self) {
        // SAFETY: zero exactly the `KEY_LEN` bytes we initialized, via a
        // volatile write so the optimizer can't elide it as a dead store
        // right before `dealloc`. `_lock` (if present) is unlocked
        // automatically right after this function returns, as part of the
        // struct's normal field-drop sequence.
        unsafe {
            for i in 0..KEY_LEN {
                std::ptr::write_volatile(self.ptr.as_ptr().add(i), 0);
            }
            dealloc(self.ptr.as_ptr(), self.layout);
        }
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

    let key = VaultKey::from_bytes(out);
    out.zeroize();
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_key_round_trips_its_bytes() {
        let bytes = [7u8; KEY_LEN];
        let key = VaultKey::from_raw(bytes);
        assert_eq!(key.as_bytes(), &bytes);
    }

    #[test]
    fn vault_key_random_produces_distinct_keys() {
        let a = VaultKey::random();
        let b = VaultKey::random();
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[test]
    fn many_vault_keys_can_be_created_and_dropped_without_crashing() {
        // Each VaultKey locks its own dedicated OS page; this exercises
        // that repeatedly allocating/locking/zeroing/unlocking/freeing
        // doesn't leak, double-free, or otherwise misbehave.
        for _ in 0..500 {
            let key = VaultKey::random();
            std::hint::black_box(key.as_bytes());
        }
    }

    #[test]
    fn derive_key_is_deterministic_for_the_same_inputs() {
        let salt = [1u8; SALT_LEN];
        let params = Argon2Params::for_testing();
        let a = derive_key(b"password", &salt, params).unwrap();
        let b = derive_key(b"password", &salt, params).unwrap();
        assert_eq!(a.as_bytes(), b.as_bytes());
    }
}
