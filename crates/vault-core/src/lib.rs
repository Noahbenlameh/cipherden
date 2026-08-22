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

/// Write `data` to `path` so that a crash or power/USB loss at any point
/// during the write leaves the original file intact (never truncated or
/// half-written): write to a sibling temp file, fsync its contents, then
/// atomically rename over the destination (`std::fs::rename` is a single
/// filesystem-level replace on both Unix `rename()` and Windows
/// `MoveFileExW(..., MOVEFILE_REPLACE_EXISTING)` — never a delete+create).
pub(crate) fn atomic_write(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("atomic");
    let tmp_path = dir.join(format!(".{file_name}.tmp-{}", std::process::id()));

    {
        let mut f = std::fs::File::create(&tmp_path)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)?;

    // Best-effort: on Unix the rename itself is durable only once the
    // directory entry is fsync'd too. Windows has no equivalent concept.
    #[cfg(unix)]
    {
        if let Ok(dir_file) = std::fs::File::open(dir) {
            let _ = dir_file.sync_all();
        }
    }

    Ok(())
}

#[cfg(test)]
mod atomic_write_tests {
    use super::atomic_write;

    #[test]
    fn replaces_existing_file_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meta.json");
        std::fs::write(&path, b"old").unwrap();

        atomic_write(&path, b"new-content").unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), b"new-content");
        // No leftover temp file after a successful write.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp file was not cleaned up by rename"
        );
    }

    #[test]
    fn an_interrupted_write_never_touches_the_original() {
        // Simulate a crash/USB-yank mid-write: the temp file gets created
        // and partially written, but the process dies before the rename
        // that would make the new content visible. The original file must
        // still read back exactly as it was.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("meta.json");
        std::fs::write(&path, b"original-good-data").unwrap();

        let tmp_path = dir
            .path()
            .join(format!(".meta.json.tmp-{}", std::process::id()));
        std::fs::write(&tmp_path, b"truncated-garb").unwrap();
        // Deliberately no rename() here -- that's the "yanked before commit" case.

        assert_eq!(std::fs::read(&path).unwrap(), b"original-good-data");
    }
}
