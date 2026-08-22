//! Ledger zone: a simple running cash-flow log — a date (set automatically
//! when a transaction is recorded), a signed amount, and a comment. Amounts
//! are stored as integer cents (`i64`) rather than floating point so the
//! running total (`LedgerVault::total`) can never accumulate rounding
//! error, no matter how many rows exist.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::container;
use crate::error::{Result, VaultError};
use crate::kdf::{Argon2Params, VaultKey};
use crate::meta::meta_path_for;
use crate::now_rfc3339;

pub(crate) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS transactions (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    date         TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,
    comment      TEXT NOT NULL DEFAULT ''
);
";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: i64,
    pub date: String,
    pub amount_cents: i64,
    pub comment: String,
}

pub struct LedgerVault {
    conn: Connection,
    db_path: Option<PathBuf>,
    #[allow(dead_code)]
    key: VaultKey,
}

impl std::fmt::Debug for LedgerVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LedgerVault")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl LedgerVault {
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
    /// database, deserialized by a `Shell`) as a `LedgerVault`. No real file
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

    /// Record a transaction with `amount_cents` (positive = inflow, negative
    /// = outflow) and a free-text comment. The date is set automatically to
    /// the moment of recording — there is deliberately no way to backdate
    /// an entry.
    pub fn add_transaction(&self, amount_cents: i64, comment: &str) -> Result<i64> {
        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO transactions (date, amount_cents, comment) VALUES (?1, ?2, ?3)",
            rusqlite::params![now, amount_cents, comment],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Correct an existing transaction's amount/comment. The original date
    /// is left untouched — this fixes a typo, it doesn't backdate a new one.
    pub fn update_transaction(&self, id: i64, amount_cents: i64, comment: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE transactions SET amount_cents = ?1, comment = ?2 WHERE id = ?3",
            rusqlite::params![amount_cents, comment, id],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(id));
        }
        Ok(())
    }

    pub fn delete_transaction(&self, id: i64) -> Result<()> {
        let changed = self.conn.execute(
            "DELETE FROM transactions WHERE id = ?1",
            rusqlite::params![id],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(id));
        }
        Ok(())
    }

    /// Newest first — a personal ledger is read top-down as "what happened
    /// most recently."
    pub fn list_transactions(&self) -> Result<Vec<Transaction>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, date, amount_cents, comment FROM transactions ORDER BY id DESC")?;
        let rows = stmt.query_map([], row_to_transaction)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Sum of every transaction's amount, in cents. `0` for an empty ledger.
    pub fn total_cents(&self) -> Result<i64> {
        let total: Option<i64> =
            self.conn
                .query_row("SELECT SUM(amount_cents) FROM transactions", [], |row| {
                    row.get(0)
                })?;
        Ok(total.unwrap_or(0))
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

fn row_to_transaction(row: &rusqlite::Row) -> rusqlite::Result<Transaction> {
    Ok(Transaction {
        id: row.get(0)?,
        date: row.get(1)?,
        amount_cents: row.get(2)?,
        comment: row.get(3)?,
    })
}
