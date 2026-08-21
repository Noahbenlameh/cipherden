//! FileVault: a second, independent encrypted "zone" for storing arbitrary
//! files, alongside (not inside) the accounts `Vault`. Deliberately its own
//! container file rather than a table bolted onto the accounts database —
//! per the project's zones architecture, each zone is a separate encrypted
//! file so they can have different passwords and be copied/backed up
//! independently. Reuses the exact same Argon2id + SQLCipher approach as
//! `store::Vault`; only the schema differs (blobs instead of text fields).
//!
//! Files live inside folders (a simple tree, like a desktop), so the
//! frontend can offer an icon-grid "desktop" view with drag-and-drop
//! between folders instead of a flat table.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::container;
use crate::error::{Result, VaultError};
use crate::kdf::{Argon2Params, VaultKey};
use crate::meta::meta_path_for;
use crate::now_rfc3339;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS folders (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    parent_id   INTEGER REFERENCES folders(id),
    name        TEXT NOT NULL,
    pinned      INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS files (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id   INTEGER REFERENCES folders(id),
    name        TEXT NOT NULL,
    size        INTEGER NOT NULL,
    data        BLOB NOT NULL,
    pinned      INTEGER NOT NULL DEFAULT 0,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_files_folder ON files(folder_id);
CREATE INDEX IF NOT EXISTS idx_folders_parent ON folders(parent_id);
";

/// Metadata about a stored file, without its (potentially large) contents —
/// what listings should fetch instead of the full blob.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMeta {
    pub id: i64,
    pub folder_id: Option<i64>,
    pub name: String,
    pub size: i64,
    pub pinned: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub pinned: bool,
    pub created_at: String,
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

    // --- folders ---------------------------------------------------------

    pub fn create_folder(&self, parent_id: Option<i64>, name: &str) -> Result<i64> {
        if let Some(pid) = parent_id {
            self.require_folder_exists(pid)?;
        }
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO folders (parent_id, name, created_at) VALUES (?1, ?2, ?3)",
            rusqlite::params![parent_id, name, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List folders directly inside `parent_id` (`None` = root), pinned first.
    pub fn list_folders(&self, parent_id: Option<i64>) -> Result<Vec<Folder>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, parent_id, name, pinned, created_at FROM folders
             WHERE parent_id IS ?1
             ORDER BY pinned DESC, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(rusqlite::params![parent_id], |row| {
            Ok(Folder {
                id: row.get(0)?,
                parent_id: row.get(1)?,
                name: row.get(2)?,
                pinned: row.get::<_, i64>(3)? != 0,
                created_at: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn rename_folder(&self, id: i64, new_name: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE folders SET name = ?1 WHERE id = ?2",
            rusqlite::params![new_name, id],
        )?;
        if changed == 0 {
            return Err(VaultError::FolderNotFound(id));
        }
        Ok(())
    }

    pub fn set_folder_pinned(&self, id: i64, pinned: bool) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE folders SET pinned = ?1 WHERE id = ?2",
            rusqlite::params![pinned as i64, id],
        )?;
        if changed == 0 {
            return Err(VaultError::FolderNotFound(id));
        }
        Ok(())
    }

    /// Move a folder to a new parent (`None` = root). Refuses to create a
    /// cycle (moving a folder into itself or one of its own descendants).
    pub fn move_folder(&self, id: i64, new_parent_id: Option<i64>) -> Result<()> {
        self.require_folder_exists(id)?;
        if let Some(new_parent_id) = new_parent_id {
            self.require_folder_exists(new_parent_id)?;
            let mut cursor = Some(new_parent_id);
            while let Some(current) = cursor {
                if current == id {
                    return Err(VaultError::CyclicFolderMove);
                }
                cursor = self.conn.query_row(
                    "SELECT parent_id FROM folders WHERE id = ?1",
                    rusqlite::params![current],
                    |row| row.get::<_, Option<i64>>(0),
                )?;
            }
        }
        self.conn.execute(
            "UPDATE folders SET parent_id = ?1 WHERE id = ?2",
            rusqlite::params![new_parent_id, id],
        )?;
        Ok(())
    }

    /// Delete a folder. Refuses if it still contains files or subfolders —
    /// callers must empty it first, so nothing is silently destroyed.
    pub fn delete_folder(&self, id: i64) -> Result<()> {
        self.require_folder_exists(id)?;
        let file_count: i64 = self.conn.query_row(
            "SELECT count(*) FROM files WHERE folder_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )?;
        let subfolder_count: i64 = self.conn.query_row(
            "SELECT count(*) FROM folders WHERE parent_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )?;
        if file_count > 0 || subfolder_count > 0 {
            return Err(VaultError::FolderNotEmpty(id));
        }
        self.conn
            .execute("DELETE FROM folders WHERE id = ?1", rusqlite::params![id])?;
        Ok(())
    }

    fn require_folder_exists(&self, id: i64) -> Result<()> {
        let exists: i64 = self.conn.query_row(
            "SELECT count(*) FROM folders WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )?;
        if exists == 0 {
            return Err(VaultError::FolderNotFound(id));
        }
        Ok(())
    }

    // --- files -------------------------------------------------------------

    /// Store a file's contents under `name`, inside `folder_id` (`None` = root).
    pub fn add_file(&self, folder_id: Option<i64>, name: &str, data: &[u8]) -> Result<i64> {
        if let Some(fid) = folder_id {
            self.require_folder_exists(fid)?;
        }
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO files (folder_id, name, size, data, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![folder_id, name, data.len() as i64, data, now],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// List files directly inside `folder_id` (`None` = root), pinned first.
    /// Never loads file contents — metadata only.
    pub fn list_files(&self, folder_id: Option<i64>) -> Result<Vec<FileMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, folder_id, name, size, pinned, created_at, updated_at FROM files
             WHERE folder_id IS ?1
             ORDER BY pinned DESC, name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(rusqlite::params![folder_id], row_to_file_meta)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Fetch a file's full contents by id.
    pub fn read_file(&self, id: i64) -> Result<Option<(FileMeta, Vec<u8>)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, folder_id, name, size, pinned, created_at, updated_at, data FROM files WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let meta = row_to_file_meta(row)?;
        let data: Vec<u8> = row.get(7)?;
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

    /// Move a file into `folder_id` (`None` = root) — the drag-and-drop
    /// "drop file onto folder icon" action.
    pub fn move_file(&self, id: i64, folder_id: Option<i64>) -> Result<()> {
        if let Some(fid) = folder_id {
            self.require_folder_exists(fid)?;
        }
        let changed = self.conn.execute(
            "UPDATE files SET folder_id = ?1 WHERE id = ?2",
            rusqlite::params![folder_id, id],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(id));
        }
        Ok(())
    }

    pub fn set_file_pinned(&self, id: i64, pinned: bool) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE files SET pinned = ?1 WHERE id = ?2",
            rusqlite::params![pinned as i64, id],
        )?;
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

fn row_to_file_meta(row: &rusqlite::Row) -> rusqlite::Result<FileMeta> {
    Ok(FileMeta {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        name: row.get(2)?,
        size: row.get(3)?,
        pinned: row.get::<_, i64>(4)? != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}
