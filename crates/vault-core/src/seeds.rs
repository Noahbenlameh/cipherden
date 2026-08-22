use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::container;
use crate::error::{Result, VaultError};
use crate::kdf::{Argon2Params, VaultKey};
use crate::meta::meta_path_for;
use crate::now_rfc3339;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS seed_entries (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    label             TEXT NOT NULL,
    network           TEXT NOT NULL DEFAULT '',
    seed_phrase       TEXT NOT NULL DEFAULT '',
    derivation_path   TEXT NOT NULL DEFAULT '',
    notes             TEXT NOT NULL DEFAULT '',
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);
";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedEntry {
    pub id: i64,
    pub label: String,
    pub network: String,
    pub seed_phrase: String,
    pub derivation_path: String,
    pub notes: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NewSeedEntry {
    pub label: String,
    pub network: String,
    pub seed_phrase: String,
    pub derivation_path: String,
    pub notes: String,
}

pub struct SeedVault {
    conn: Connection,
    db_path: Option<PathBuf>,
    #[allow(dead_code)]
    key: VaultKey,
}

impl std::fmt::Debug for SeedVault {
    // Deliberately hand-written: never derive Debug here — a seed phrase is
    // exactly the kind of secret that must never end up in a log line or
    // panic message via an accidental `{:?}`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeedVault")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl SeedVault {
    pub fn create(
        db_path: impl AsRef<Path>,
        master_password: &str,
        params: Argon2Params,
    ) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let (conn, key) = container::create(&db_path, master_password, params, SCHEMA)?;
        Ok(Self {
            conn,
            db_path: Some(db_path),
            key,
        })
    }

    pub fn open(db_path: impl AsRef<Path>, master_password: &str) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        let (conn, key) = container::open(&db_path, master_password, SCHEMA)?;
        Ok(Self {
            conn,
            db_path: Some(db_path),
            key,
        })
    }

    /// Wrap an already-open connection (an embedded zone's in-memory
    /// database, deserialized by a `Shell`) as a `SeedVault`. No real file
    /// behind this — `db_path()` returns `None`.
    pub(crate) fn wrap(conn: Connection, key: VaultKey) -> Self {
        Self {
            conn,
            db_path: None,
            key,
        }
    }

    pub fn db_path(&self) -> Option<&Path> {
        self.db_path.as_deref()
    }

    pub(crate) fn serialize(&self) -> Result<Vec<u8>> {
        container::serialize_to_bytes(&self.conn)
    }

    pub(crate) fn key(&self) -> &VaultKey {
        &self.key
    }

    pub fn add_seed(&self, entry: &NewSeedEntry) -> Result<i64> {
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO seed_entries
                (label, network, seed_phrase, derivation_path, notes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            rusqlite::params![
                entry.label,
                entry.network,
                entry.seed_phrase,
                entry.derivation_path,
                entry.notes,
                now,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn get_seed(&self, id: i64) -> Result<Option<SeedEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, label, network, seed_phrase, derivation_path, notes, created_at, updated_at
             FROM seed_entries WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_seed(row)?))
        } else {
            Ok(None)
        }
    }

    pub fn update_seed(&self, id: i64, entry: &NewSeedEntry) -> Result<()> {
        let now = now_rfc3339();
        let changed = self.conn.execute(
            "UPDATE seed_entries SET label = ?1, network = ?2, seed_phrase = ?3,
                derivation_path = ?4, notes = ?5, updated_at = ?6
             WHERE id = ?7",
            rusqlite::params![
                entry.label,
                entry.network,
                entry.seed_phrase,
                entry.derivation_path,
                entry.notes,
                now,
                id,
            ],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(id));
        }
        Ok(())
    }

    pub fn delete_seed(&self, id: i64) -> Result<()> {
        let changed = self.conn.execute(
            "DELETE FROM seed_entries WHERE id = ?1",
            rusqlite::params![id],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(id));
        }
        Ok(())
    }

    pub fn list_seeds(&self) -> Result<Vec<SeedEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, label, network, seed_phrase, derivation_path, notes, created_at, updated_at
             FROM seed_entries ORDER BY label COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], row_to_seed)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn export_backup(&self, dest_dir: impl AsRef<Path>) -> Result<PathBuf> {
        let db_path = self.db_path.as_ref().ok_or_else(|| {
            VaultError::InvalidMeta(
                "this vault is an embedded zone with no backing file to back up directly \
                 (use the Shell's own export/backup instead)"
                    .into(),
            )
        })?;
        let dest_dir = dest_dir.as_ref();
        std::fs::create_dir_all(dest_dir)?;

        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;

        let file_name = db_path
            .file_name()
            .ok_or_else(|| VaultError::InvalidMeta("vault path has no file name".into()))?;
        let dest_db = dest_dir.join(file_name);
        std::fs::copy(db_path, &dest_db)?;

        let src_meta = meta_path_for(db_path);
        let dest_meta = meta_path_for(&dest_db);
        std::fs::copy(&src_meta, &dest_meta)?;

        Ok(dest_db)
    }
}

fn row_to_seed(row: &rusqlite::Row) -> rusqlite::Result<SeedEntry> {
    Ok(SeedEntry {
        id: row.get(0)?,
        label: row.get(1)?,
        network: row.get(2)?,
        seed_phrase: row.get(3)?,
        derivation_path: row.get(4)?,
        notes: row.get(5)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}
