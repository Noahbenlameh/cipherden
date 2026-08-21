//! FileVault: a second, independent encrypted "zone" for storing arbitrary
//! files, alongside (not inside) the accounts `Vault`. Deliberately its own
//! container file rather than a table bolted onto the accounts database —
//! per the project's zones architecture, each zone is a separate encrypted
//! file so they can have different passwords and be copied/backed up
//! independently. Reuses the exact same Argon2id + SQLCipher approach as
//! `store::Vault`; only the schema differs (blobs instead of text fields).

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::container;
use crate::error::{Result, VaultError};
use crate::kdf::{Argon2Params, VaultKey};
use crate::meta::meta_path_for;
use crate::now_rfc3339;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS files (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    data        BLOB NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
";

/// Metadata about a stored file, without its (potentially large) contents —
/// what listings should fetch instead of the full blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub created_at: String,
    pub updated_at: String,
}

pub struct FileVault {
    conn: Connection,
    db_path: PathBuf,
    #[allow(dead_code)]
    key: VaultKey,
}

impl std::fmt::Debug for FileVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileVault")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl FileVault {
    pub fn create(
        db_path: impl AsRef<Path>,
        master_password: &str,
        params: Argon2Params,
    ) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let (conn, key) = container::create(&db_path, master_password, params, SCHEMA)?;
        Ok(Self { conn, db_path, key })
    }

    pub fn open(db_path: impl AsRef<Path>, master_password: &str) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let (conn, key) = container::open(&db_path, master_password, SCHEMA)?;
        Ok(Self { conn, db_path, key })
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Store a file's contents under `name`. Returns the new file's id.
    pub fn add_file(&self, name: &str, data: &[u8]) -> Result<i64> {
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO files (name, size, data, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
            rusqlite::params![name, data.len() as i64, data, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List stored files (metadata only — never loads file contents).
    pub fn list_files(&self) -> Result<Vec<FileMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, size, created_at, updated_at FROM files ORDER BY name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(FileMeta {
                id: row.get(0)?,
                name: row.get(1)?,
                size: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Fetch a file's full contents by id.
    pub fn read_file(&self, id: i64) -> Result<Option<(FileMeta, Vec<u8>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, size, created_at, updated_at, data FROM files WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let meta = FileMeta {
            id: row.get(0)?,
            name: row.get(1)?,
            size: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
        };
        let data: Vec<u8> = row.get(5)?;
        Ok(Some((meta, data)))
    }

    pub fn delete_file(&self, id: i64) -> Result<()> {
        let changed = self
            .conn
            .execute("DELETE FROM files WHERE id = ?1", rusqlite::params![id])?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(id));
        }
        Ok(())
    }

    /// Same "copy the whole encrypted container elsewhere" backup story as
    /// the accounts vault.
    pub fn export_backup(&self, dest_dir: impl AsRef<Path>) -> Result<PathBuf> {
        let dest_dir = dest_dir.as_ref();
        std::fs::create_dir_all(dest_dir)?;
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
