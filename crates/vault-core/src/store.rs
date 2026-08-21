use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::container;
use crate::error::{Result, VaultError};
use crate::kdf::{Argon2Params, VaultKey};
use crate::meta::meta_path_for;
use crate::now_rfc3339;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    title       TEXT NOT NULL,
    username    TEXT NOT NULL DEFAULT '',
    password    TEXT NOT NULL DEFAULT '',
    url         TEXT NOT NULL DEFAULT '',
    notes       TEXT NOT NULL DEFAULT '',
    category    TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_entries_category ON entries(category);
";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: i64,
    pub title: String,
    pub username: String,
    pub password: String,
    pub url: String,
    pub notes: String,
    pub category: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewEntry {
    pub title: String,
    pub username: String,
    pub password: String,
    pub url: String,
    pub notes: String,
    pub category: String,
}

pub struct Vault {
    conn: Connection,
    db_path: PathBuf,
    #[allow(dead_code)] // retained so the key stays alive/zeroizable for the vault's lifetime
    key: VaultKey,
}

impl std::fmt::Debug for Vault {
    // Deliberately hand-written: never derive Debug here, so the key field
    // can never accidentally end up in a log line or panic message.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Create a brand-new vault at `db_path`. Fails if a file already exists
    /// there. `params` should almost always be `Argon2Params::standard()`.
    pub fn create(
        db_path: impl AsRef<Path>,
        master_password: &str,
        params: Argon2Params,
    ) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let (conn, key) = container::create(&db_path, master_password, params, SCHEMA)?;
        Ok(Self { conn, db_path, key })
    }

    /// Open an existing vault. Returns `VaultError::InvalidPassword` if the
    /// password is wrong (this is indistinguishable from the db file being
    /// corrupted, which is the correct behavior for an authenticated cipher).
    pub fn open(db_path: impl AsRef<Path>, master_password: &str) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let (conn, key) = container::open(&db_path, master_password, SCHEMA)?;
        Ok(Self { conn, db_path, key })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn add_entry(&self, entry: &NewEntry) -> Result<i64> {
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO entries (title, username, password, url, notes, category, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            rusqlite::params![
                entry.title,
                entry.username,
                entry.password,
                entry.url,
                entry.notes,
                entry.category,
                now,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_entry(&self, id: i64) -> Result<Option<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, username, password, url, notes, category, created_at, updated_at
             FROM entries WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_entry(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_entry(&self, id: i64, entry: &NewEntry) -> Result<()> {
        let now = now_rfc3339();
        let changed = self.conn.execute(
            "UPDATE entries SET title = ?1, username = ?2, password = ?3, url = ?4,
                notes = ?5, category = ?6, updated_at = ?7
             WHERE id = ?8",
            rusqlite::params![
                entry.title,
                entry.username,
                entry.password,
                entry.url,
                entry.notes,
                entry.category,
                now,
                id,
            ],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(id));
        }
        Ok(())
    }

    pub fn delete_entry(&self, id: i64) -> Result<()> {
        let changed = self
            .conn
            .execute("DELETE FROM entries WHERE id = ?1", rusqlite::params![id])?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(id));
        }
        Ok(())
    }

    pub fn list_entries(&self) -> Result<Vec<Entry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, username, password, url, notes, category, created_at, updated_at
             FROM entries ORDER BY title COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], row_to_entry)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Case-insensitive substring search over title, username, url, category.
    pub fn search(&self, query: &str) -> Result<Vec<Entry>> {
        let pattern = format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"));
        let mut stmt = self.conn.prepare(
            "SELECT id, title, username, password, url, notes, category, created_at, updated_at
             FROM entries
             WHERE title LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                OR username LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                OR url LIKE ?1 ESCAPE '\\' COLLATE NOCASE
                OR category LIKE ?1 ESCAPE '\\' COLLATE NOCASE
             ORDER BY title COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(rusqlite::params![pattern], row_to_entry)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Export a full encrypted backup: the SQLCipher file plus its
    /// (non-secret) metadata sidecar, byte-for-byte, opened later with the
    /// same master password. This is the "3-2-1 backup" button from the
    /// spec's mandatory disk-failure protection section — not optional.
    pub fn export_backup(&self, dest_dir: impl AsRef<Path>) -> Result<PathBuf> {
        let dest_dir = dest_dir.as_ref();
        std::fs::create_dir_all(dest_dir)?;

        // Flush WAL/journal so the on-disk file is self-contained before copying.
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;

        let file_name = self
            .db_path
            .file_name()
            .ok_or_else(|| VaultError::InvalidMeta("vault path has no file name".into()))?;
        let dest_db = dest_dir.join(file_name);
        std::fs::copy(&self.db_path, &dest_db)?;

        let src_meta = meta_path_for(&self.db_path);
        let dest_meta = meta_path_for(&dest_db);
        std::fs::copy(&src_meta, &dest_meta)?;

        Ok(dest_db)
    }
}

fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<Entry> {
    Ok(Entry {
        id: row.get(0)?,
        title: row.get(1)?,
        username: row.get(2)?,
        password: row.get(3)?,
        url: row.get(4)?,
        notes: row.get(5)?,
        category: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
