//! vault-core: encrypted storage core for CIPHERDEN.
//!
//! This crate has no UI dependencies and is responsible for exactly one
//! thing: turning a master password plus a directory on disk into a durable,
//! authenticated, encrypted store of password entries.
//!
//! Cryptographic primitives are never implemented here — only wired up from
//! vetted libraries: Argon2id via the RustCrypto `argon2` crate for the KDF,
//! and SQLCipher (via `rusqlite`'s `bundled-sqlcipher` feature) for
//! authenticated AES-256 encryption at rest.

mod container;
pub mod error;
pub mod export;
pub mod files;
pub mod import;
pub mod kdf;
mod keyslots;
pub mod ledger;
pub mod meta;
pub mod seeds;
pub mod shell;
pub mod store;

pub use error::{Result, VaultError};
pub use files::FileVault;
pub use kdf::Argon2Params;
pub use ledger::{LedgerVault, Transaction};
pub use seeds::{NewSeedEntry, SeedEntry, SeedVault};
pub use shell::{OpenZone, Shell, ZoneKind, ZoneMeta, PRIMARY_SLOT, RECOVERY_SLOT};
pub use store::{Entry, NewEntry, Vault};

pub(crate) fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("RFC3339 formatting of a valid OffsetDateTime cannot fail")
}
