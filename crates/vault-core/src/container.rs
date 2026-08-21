//! Shared "open/create an encrypted SQLCipher container" logic used by
//! every zone (`store::Vault`, `files::FileVault`, and future zones). Each
//! zone only supplies its own SQL schema; the KDF, keying, and
//! wrong-password detection are identical everywhere and must stay that way.

use std::path::Path;

use rusqlite::Connection;

use crate::error::{Result, VaultError};
use crate::kdf::{derive_key, random_salt, Argon2Params, VaultKey};
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
