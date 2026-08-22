//! Shared "open/create an encrypted SQLCipher container" logic used by
//! every zone (`store::Vault`, `files::FileVault`, and future zones). Each
//! zone only supplies its own SQL schema; the KDF, keying, and
//! wrong-password detection are identical everywhere and must stay that way.

use std::path::Path;
use std::ptr::NonNull;

use rand::RngCore;
use rusqlite::serialize::OwnedData;
use rusqlite::{ffi, Connection};

use crate::error::{Result, VaultError};
use crate::kdf::{derive_key, random_salt, Argon2Params, VaultKey, SALT_LEN};
use crate::meta::{meta_path_for, VaultMeta};

pub(crate) fn create(
    db_path: &Path,
    master_password: &str,
    params: Argon2Params,
    schema: &str,
) -> Result<(Connection, VaultKey)> {
    if db_path.exists() {
        return Err(VaultError::AlreadyExists(db_path.to_path_buf()));
    }
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let salt = random_salt();
    let key = derive_key(master_password.as_bytes(), &salt, params)?;

    let meta = VaultMeta::new(&salt, params);
    meta.save(&meta_path_for(db_path))?;

    let conn = Connection::open(db_path)?;
    set_key(&conn, &key)?;
    conn.execute_batch(schema)?;

    Ok((conn, key))
}

pub(crate) fn open(
    db_path: &Path,
    master_password: &str,
    schema: &str,
) -> Result<(Connection, VaultKey)> {
    if !db_path.exists() {
        return Err(VaultError::NotFound(db_path.to_path_buf()));
    }
    let meta_path = meta_path_for(db_path);
    if !meta_path.exists() {
        return Err(VaultError::InvalidMeta(format!(
            "missing metadata file: {}",
            meta_path.display()
        )));
    }
    let meta = VaultMeta::load(&meta_path)?;
    let salt = meta.salt_bytes()?;
    let key = derive_key(master_password.as_bytes(), &salt, meta.argon2)?;

    let conn = Connection::open(db_path)?;
    set_key(&conn, &key)?;

    // SQLCipher only reveals a wrong key when it actually tries to parse a
    // page. Reading sqlite_master forces that and gives us a clean single
    // point to translate into "wrong password".
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|_| VaultError::InvalidPassword)?;
    conn.execute_batch(schema)?; // idempotent (CREATE TABLE IF NOT EXISTS); heals an interrupted first create

    Ok((conn, key))
}

// --- File-backed containers keyed by an already-derived key -------------
//
// Used by `Shell`, whose real SQLCipher key is a randomly-generated data
// key (wrapped under one or more passwords via `keyslots.rs`), not derived
// directly from a single password the way every other file-backed
// container above is. No sidecar `.meta.json` is written here — the Shell
// persists its `KeySlots` (which include each slot's own salt/params) to
// that same sidecar path itself.

pub(crate) fn create_with_key(db_path: &Path, key: &VaultKey, schema: &str) -> Result<Connection> {
    if db_path.exists() {
        return Err(VaultError::AlreadyExists(db_path.to_path_buf()));
    }
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    set_key(&conn, key)?;
    conn.execute_batch(schema)?;
    Ok(conn)
}

pub(crate) fn open_with_key(db_path: &Path, key: &VaultKey, schema: &str) -> Result<Connection> {
    if !db_path.exists() {
        return Err(VaultError::NotFound(db_path.to_path_buf()));
    }
    let conn = Connection::open(db_path)?;
    set_key(&conn, key)?;
    conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
        row.get::<_, i64>(0)
    })
    .map_err(|_| VaultError::InvalidPassword)?;
    conn.execute_batch(schema)?;
    Ok(conn)
}

// --- In-memory containers: for zones embedded inside a Shell -------------
//
// These never touch disk. A zone's serialized bytes live only as a BLOB
// inside the Shell's own (file-backed) database; there is deliberately no
// per-zone sidecar file, so the zone's very existence is hidden until the
// Shell itself is unlocked. The Shell persists the KDF salt/params itself
// (see `shell.rs`) since there's no sidecar file here to hold them.

/// Create a brand-new zone database entirely in memory. Returns the
/// connection, its derived key, and the freshly-generated salt — the
/// caller (the Shell) is responsible for persisting the salt/params.
///
/// Deliberately does **not** use SQLCipher's `PRAGMA key` here — verified
/// directly (during development, before settling on this design) that
/// SQLCipher's per-page codec is not applied to `:memory:` connections at
/// all, silently leaving data unencrypted. Confidentiality for embedded
/// zones instead comes from whole-blob AES-256-GCM
/// (`encrypt_blob`/`decrypt_blob`) applied when the zone is serialized out
/// to be stored in the Shell.
pub(crate) fn create_in_memory(
    master_password: &str,
    params: Argon2Params,
    schema: &str,
) -> Result<(Connection, VaultKey, [u8; SALT_LEN])> {
    let salt = random_salt();
    let key = derive_key(master_password.as_bytes(), &salt, params)?;

    let conn = Connection::open_in_memory()?;
    conn.execute_batch(schema)?;

    Ok((conn, key, salt))
}

/// Serialize an in-memory zone connection to raw (unencrypted) bytes. The
/// caller must pass this through `encrypt_blob` before persisting it
/// anywhere.
pub(crate) fn serialize_to_bytes(conn: &Connection) -> Result<Vec<u8>> {
    let data = conn.serialize("main")?;
    Ok(data.to_vec())
}

/// Load a zone from its encrypted blob (fetched from the Shell's blob
/// column) into a fresh in-memory connection. Never touches disk. Wrong
/// password / corrupted data both surface as `VaultError::InvalidPassword`,
/// via the AEAD tag check in `decrypt_blob` — same "can't tell them apart"
/// property every other zone already has.
pub(crate) fn open_in_memory(
    encrypted_blob: &[u8],
    master_password: &str,
    salt: &[u8; SALT_LEN],
    params: Argon2Params,
) -> Result<(Connection, VaultKey)> {
    let key = derive_key(master_password.as_bytes(), salt, params)?;
    let plaintext = decrypt_blob(&key, encrypted_blob)?;

    let mut conn = Connection::open_in_memory()?;
    conn.deserialize("main", owned_data_from_bytes(&plaintext), false)?;

    Ok((conn, key))
}

const GCM_NONCE_LEN: usize = 12;

/// Encrypts an entire serialized zone database as one opaque blob
/// (AES-256-GCM, random 96-bit nonce prepended to the ciphertext). This —
/// not SQLCipher's codec, which doesn't apply to `:memory:` connections —
/// is what actually protects an embedded zone's data at rest inside the
/// Shell's file.
pub(crate) fn encrypt_blob(key: &VaultKey, plaintext: &[u8]) -> Vec<u8> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("key is always 32 bytes");
    let mut nonce_bytes = [0u8; GCM_NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(&Nonce::from(nonce_bytes), plaintext)
        .expect("AES-GCM encryption of an in-memory buffer cannot fail");

    let mut out = Vec::with_capacity(GCM_NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    out
}

/// Inverse of `encrypt_blob`. A wrong key or any tampering both fail the
/// AEAD tag check, surfaced as `VaultError::InvalidPassword`.
pub(crate) fn decrypt_blob(key: &VaultKey, data: &[u8]) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};

    if data.len() < GCM_NONCE_LEN {
        return Err(VaultError::InvalidPassword);
    }
    let (nonce_bytes, ciphertext) = data.split_at(GCM_NONCE_LEN);
    let nonce: [u8; GCM_NONCE_LEN] = nonce_bytes
        .try_into()
        .expect("split_at guarantees this length");
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes()).expect("key is always 32 bytes");
    cipher
        .decrypt(&Nonce::from(nonce), ciphertext)
        .map_err(|_| VaultError::InvalidPassword)
}

/// `Connection::deserialize` requires a buffer allocated by
/// `sqlite3_malloc` (it will `sqlite3_free` it later) — copy our
/// ordinarily-allocated bytes (e.g. read back from a SQLite BLOB column)
/// into one.
fn owned_data_from_bytes(bytes: &[u8]) -> OwnedData {
    unsafe {
        let ptr = ffi::sqlite3_malloc64(bytes.len() as u64).cast::<u8>();
        let ptr = NonNull::new(ptr).expect("sqlite3_malloc64 allocation failed");
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.as_ptr(), bytes.len());
        OwnedData::from_raw_nonnull(ptr, bytes.len())
    }
}

/// Export a currently-open embedded zone connection to a brand-new,
/// independently-openable, SQLCipher-encrypted file at `dest_path` —
/// keyed by its own freshly-derived Argon2id key (a new random salt,
/// written to the usual `.meta.json` sidecar), *not* the embedded zone's
/// in-memory key. `password` is typically the zone's own current password,
/// so the exported file opens later with the same password the user
/// already knows, via the zone type's ordinary `open()` (e.g.
/// `Vault::open`) — this is the opt-in safety valve for someone who wants
/// one zone's data recoverable even without the Shell.
///
/// Implemented via SQLCipher's `sqlcipher_export()`: `conn` (the embedded
/// zone, unencrypted at the SQLite layer — see the module doc comment on
/// why) attaches the new encrypted file and copies every table into it in
/// one step, rather than us re-deriving a manual table-by-table copy.
pub(crate) fn export_in_memory_to_encrypted_file(
    conn: &Connection,
    dest_path: &Path,
    password: &str,
    params: Argon2Params,
) -> Result<()> {
    if dest_path.exists() {
        return Err(VaultError::AlreadyExists(dest_path.to_path_buf()));
    }
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let salt = random_salt();
    let key = derive_key(password.as_bytes(), &salt, params)?;

    let meta = VaultMeta::new(&salt, params);
    meta.save(&meta_path_for(dest_path))?;

    let dest_path_str = dest_path
        .to_str()
        .ok_or_else(|| VaultError::InvalidMeta("destination path is not valid UTF-8".into()))?;

    // The KEY clause needs the same "raw hex key" literal syntax `set_key`
    // uses for `PRAGMA key` — a bound BLOB parameter here is not recognized
    // as raw key material by SQLCipher and silently derives a different key
    // instead, which was caught by a round-trip test failing with
    // InvalidPassword. The hex digest is always `[0-9a-f]{64}`, so
    // interpolating it directly into the SQL text carries no injection risk.
    let hex_key = to_hex(key.as_bytes());
    conn.execute(
        &format!("ATTACH DATABASE ?1 AS export_target KEY \"x'{hex_key}'\""),
        rusqlite::params![dest_path_str],
    )?;
    let export_result = conn.execute_batch("SELECT sqlcipher_export('export_target');");
    // Always try to detach, even if the export failed, so a retry (or the
    // next unrelated use of this connection) doesn't trip over a stuck
    // attachment.
    let detach_result = conn.execute_batch("DETACH DATABASE export_target;");
    export_result?;
    detach_result?;

    Ok(())
}

fn set_key(conn: &Connection, key: &VaultKey) -> Result<()> {
    let hex_key = to_hex(key.as_bytes());
    // Raw key form (x'...') hands SQLCipher our already-derived Argon2id key
    // directly, so SQLCipher does not additionally run its own (weaker,
    // PBKDF2-based) passphrase KDF on top.
    conn.execute_batch(&format!("PRAGMA key = \"x'{hex_key}'\";"))?;
    Ok(())
}

fn to_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(s, "{b:02x}").expect("writing to a String cannot fail");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SCHEMA: &str = "CREATE TABLE t (v TEXT NOT NULL);";

    /// Full pipeline every embedded zone actually goes through: create in
    /// memory -> mutate -> serialize -> encrypt (what gets stored in the
    /// Shell) -> decrypt -> deserialize -> read back.
    #[test]
    fn embedded_zone_round_trip_through_encrypted_blob() {
        let params = Argon2Params::for_testing();

        let (conn, key, salt) = create_in_memory("pw", params, TEST_SCHEMA).unwrap();
        conn.execute("INSERT INTO t (v) VALUES ('hello')", [])
            .unwrap();
        let plaintext = serialize_to_bytes(&conn).unwrap();
        let encrypted = encrypt_blob(&key, &plaintext);
        drop(conn);

        assert!(!encrypted.is_empty());
        // The whole point: the stored form must not contain the plaintext.
        assert!(!String::from_utf8_lossy(&encrypted).contains("hello"));

        let (conn2, _key2) = open_in_memory(&encrypted, "pw", &salt, params).unwrap();
        let value: String = conn2
            .query_row("SELECT v FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(value, "hello");
    }

    #[test]
    fn embedded_zone_wrong_password_is_rejected() {
        let params = Argon2Params::for_testing();
        let (conn, key, salt) = create_in_memory("correct-pw", params, TEST_SCHEMA).unwrap();
        let plaintext = serialize_to_bytes(&conn).unwrap();
        let encrypted = encrypt_blob(&key, &plaintext);
        drop(conn);

        match open_in_memory(&encrypted, "wrong-pw", &salt, params) {
            Err(VaultError::InvalidPassword) => {}
            Ok(_) => panic!("expected InvalidPassword, got Ok"),
            Err(e) => panic!("expected InvalidPassword, got {e}"),
        }
    }

    #[test]
    fn embedded_zone_tampered_blob_is_rejected() {
        let params = Argon2Params::for_testing();
        let (conn, key, salt) = create_in_memory("pw", params, TEST_SCHEMA).unwrap();
        let plaintext = serialize_to_bytes(&conn).unwrap();
        let mut encrypted = encrypt_blob(&key, &plaintext);
        drop(conn);

        let last = encrypted.len() - 1;
        encrypted[last] ^= 0xFF; // flip a bit in the ciphertext/tag

        match open_in_memory(&encrypted, "pw", &salt, params) {
            Err(VaultError::InvalidPassword) => {}
            Ok(_) => panic!("expected InvalidPassword (tamper must be caught), got Ok"),
            Err(e) => panic!("expected InvalidPassword, got {e}"),
        }
    }

    #[test]
    fn embedded_zone_never_touches_disk_and_survives_many_rows() {
        let params = Argon2Params::for_testing();
        let (conn, key, salt) = create_in_memory("pw", params, TEST_SCHEMA).unwrap();
        for i in 0..50 {
            conn.execute(
                "INSERT INTO t (v) VALUES (?1)",
                rusqlite::params![format!("row-{i}")],
            )
            .unwrap();
        }
        let plaintext = serialize_to_bytes(&conn).unwrap();
        let encrypted = encrypt_blob(&key, &plaintext);
        drop(conn);

        let (conn2, _key2) = open_in_memory(&encrypted, "pw", &salt, params).unwrap();
        let count: i64 = conn2
            .query_row("SELECT count(*) FROM t", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 50);
    }

    #[test]
    fn encrypt_blob_never_reuses_a_nonce() {
        // Not exhaustive, but catches a broken RNG or a hardcoded nonce
        // immediately — reusing a nonce with the same key is the one
        // mistake that would actually break AES-GCM's guarantees.
        let params = Argon2Params::for_testing();
        let (_conn, key, _salt) = create_in_memory("pw", params, TEST_SCHEMA).unwrap();
        let a = encrypt_blob(&key, b"same plaintext");
        let b = encrypt_blob(&key, b"same plaintext");
        assert_ne!(
            a[..GCM_NONCE_LEN],
            b[..GCM_NONCE_LEN],
            "nonce must differ per encryption"
        );
        assert_ne!(a, b);
    }
}
