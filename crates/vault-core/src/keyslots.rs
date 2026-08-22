//! Multi-slot key wrapping — lets one data-encryption key be unlocked by
//! any of several independent passwords (e.g. a primary and a recovery
//! password), where any known password can also be used to set a new one
//! for any slot, without ever writing a plaintext recovery code down
//! anywhere. This is the same principle full-disk-encryption tools (LUKS,
//! BitLocker, FileVault) use for their own "recovery key" slots, and is
//! the safe way to do what a low-entropy secondary factor (like a
//! remembered gesture/pattern) cannot: two slots are only as strong as the
//! *weaker* of the two passwords, so both must be real passwords with real
//! entropy — this deliberately does not support a short PIN/pattern slot.
//!
//! Each slot independently: generates its own random salt, derives a
//! key-encryption key (KEK) via Argon2id from that slot's password, and
//! uses it to AES-256-GCM-wrap the *same* underlying data key
//! (`container::encrypt_blob`/`decrypt_blob` — the identical primitive
//! embedded zones already use to wrap their serialized bytes). Unlocking
//! tries a candidate password against every slot; the AEAD tag check
//! naturally rejects the wrong slots without needing to know in advance
//! which slot a password belongs to.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::container::{decrypt_blob, encrypt_blob};
use crate::error::{Result, VaultError};
use crate::kdf::{derive_key, random_salt, Argon2Params, VaultKey, KEY_LEN, SALT_LEN};
use crate::meta::{base64_decode, base64_encode};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KeySlotRecord {
    name: String,
    salt: String, // base64
    argon2: Argon2Params,
    wrapped_key: String, // base64 of (nonce || AES-256-GCM ciphertext)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KeySlots {
    slots: Vec<KeySlotRecord>,
}

impl KeySlots {
    pub fn new() -> Self {
        Self { slots: Vec::new() }
    }

    /// Add a new slot, or replace an existing one with the same `name`,
    /// wrapping `data_key` under `password`.
    pub fn set_slot(
        &mut self,
        name: &str,
        password: &str,
        params: Argon2Params,
        data_key: &VaultKey,
    ) -> Result<()> {
        let salt = random_salt();
        let kek = derive_key(password.as_bytes(), &salt, params)?;
        let wrapped = encrypt_blob(&kek, data_key.as_bytes());

        let record = KeySlotRecord {
            name: name.to_string(),
            salt: base64_encode(&salt),
            argon2: params,
            wrapped_key: base64_encode(&wrapped),
        };
        self.slots.retain(|s| s.name != name);
        self.slots.push(record);
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&data)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    /// Try `password` against every slot, regardless of name. The first
    /// whose AEAD tag verifies wins; returns the unwrapped data key. A
    /// password that matches no slot (or a corrupt slot) surfaces as
    /// `VaultError::InvalidPassword`, same as everywhere else in this
    /// crate. Used where *any* currently-valid password should be
    /// accepted — e.g. authenticating a password-change request.
    pub fn unlock_any(&self, password: &str) -> Result<VaultKey> {
        self.unlock_named(password, None)
    }

    /// Like `unlock_any`, but only considers slots whose name is in
    /// `allowed`. Used to make a slot (e.g. a recovery password) unable to
    /// open the Shell directly — only usable through an explicit recovery
    /// action that calls `unlock_any` instead.
    pub fn unlock_one_of(&self, password: &str, allowed: &[&str]) -> Result<VaultKey> {
        self.unlock_named(password, Some(allowed))
    }

    fn unlock_named(&self, password: &str, allowed: Option<&[&str]>) -> Result<VaultKey> {
        for slot in &self.slots {
            if let Some(allowed) = allowed {
                if !allowed.contains(&slot.name.as_str()) {
                    continue;
                }
            }
            let Ok(salt_bytes) = base64_decode(&slot.salt) else {
                continue;
            };
            let Ok(salt) = <[u8; SALT_LEN]>::try_from(salt_bytes.as_slice()) else {
                continue;
            };
            let Ok(kek) = derive_key(password.as_bytes(), &salt, slot.argon2) else {
                continue;
            };
            let Ok(wrapped) = base64_decode(&slot.wrapped_key) else {
                continue;
            };
            let Ok(data_key_bytes) = decrypt_blob(&kek, &wrapped) else {
                continue;
            };
            let Ok(arr) = <[u8; KEY_LEN]>::try_from(data_key_bytes.as_slice()) else {
                continue;
            };
            return Ok(VaultKey::from_raw(arr));
        }
        Err(VaultError::InvalidPassword)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> Argon2Params {
        Argon2Params::for_testing()
    }

    #[test]
    fn either_slot_unlocks_the_same_data_key() {
        let data_key = VaultKey::random();
        let mut slots = KeySlots::new();
        slots
            .set_slot("primary", "pw-a", params(), &data_key)
            .unwrap();
        slots
            .set_slot("recovery", "pw-b", params(), &data_key)
            .unwrap();

        let unlocked_a = slots.unlock_any("pw-a").unwrap();
        let unlocked_b = slots.unlock_any("pw-b").unwrap();
        assert_eq!(unlocked_a.as_bytes(), data_key.as_bytes());
        assert_eq!(unlocked_b.as_bytes(), data_key.as_bytes());
    }

    #[test]
    fn unknown_password_is_rejected() {
        let data_key = VaultKey::random();
        let mut slots = KeySlots::new();
        slots
            .set_slot("primary", "pw-a", params(), &data_key)
            .unwrap();

        match slots.unlock_any("not-it") {
            Err(VaultError::InvalidPassword) => {}
            _ => panic!("expected InvalidPassword"),
        }
    }

    #[test]
    fn set_slot_replaces_a_slot_with_the_same_name() {
        let data_key = VaultKey::random();
        let mut slots = KeySlots::new();
        slots
            .set_slot("primary", "old-pw", params(), &data_key)
            .unwrap();
        slots
            .set_slot("primary", "new-pw", params(), &data_key)
            .unwrap();

        assert!(slots.unlock_any("new-pw").is_ok());
        match slots.unlock_any("old-pw") {
            Err(VaultError::InvalidPassword) => {}
            _ => panic!("old password should no longer work after replacing the slot"),
        }
    }

    #[test]
    fn unlock_one_of_ignores_disallowed_slots() {
        let data_key = VaultKey::random();
        let mut slots = KeySlots::new();
        slots
            .set_slot("primary", "pw-a", params(), &data_key)
            .unwrap();
        slots
            .set_slot("recovery", "pw-b", params(), &data_key)
            .unwrap();

        // "recovery"'s password is real and correct, but not in the
        // allowed list -- must be rejected exactly like a wrong password.
        match slots.unlock_one_of("pw-b", &["primary"]) {
            Err(VaultError::InvalidPassword) => {}
            _ => panic!("expected InvalidPassword for a disallowed slot"),
        }
        // The allowed slot's own password still works.
        assert!(slots.unlock_one_of("pw-a", &["primary"]).is_ok());
        // And unlock_any is unaffected -- still tries everything.
        assert!(slots.unlock_any("pw-b").is_ok());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slots.json");
        let data_key = VaultKey::random();

        let mut slots = KeySlots::new();
        slots
            .set_slot("primary", "pw-a", params(), &data_key)
            .unwrap();
        slots
            .set_slot("recovery", "pw-b", params(), &data_key)
            .unwrap();
        slots.save(&path).unwrap();

        let loaded = KeySlots::load(&path).unwrap();
        let unlocked = loaded.unlock_any("pw-b").unwrap();
        assert_eq!(unlocked.as_bytes(), data_key.as_bytes());
    }
}
