//! Shell: the single visible, file-backed encrypted container that every
//! "embedded" zone (Accounts, Files, Seeds, Ledger) lives inside of. Without
//! the Shell's own password, an attacker who finds the drive sees nothing —
//! not even that a "Seeds" zone exists.
//!
//! Each zone is stored as an AES-256-GCM-encrypted blob (see
//! `container::encrypt_blob`/`decrypt_blob`), protected by its own
//! independent Argon2id-derived password. The Shell password only unlocks
//! *visibility of the zone list* (labels/icons) — it grants no access to
//! any zone's actual contents. Losing the Shell password loses access to
//! everything (this is a deliberate, user-confirmed trade-off: real
//! existence-hiding over a standalone-file fallback — see PROJECT_MAP.md).
//!
//! Zones are deliberately *not* SQLCipher-keyed in memory: SQLCipher's
//! per-page codec does not apply to `:memory:` connections at all (this was
//! verified directly — see `container`'s tests — before committing to this
//! design), so whole-blob AES-256-GCM is what actually protects a zone's
//! data at rest inside the Shell.
//!
//! The Shell's own key is not derived directly from a single password
//! either: it's a randomly-generated data key, independently wrapped under
//! one password (`"primary"`) or, optionally, two (`"primary"` and
//! `"recovery"` key slots — see `keyslots.rs`) so that either one unlocks
//! the Shell, and either one can be used to set a new value for the
//! *other* slot. Creating with a recovery slot is the safer default; a
//! single-slot "strict" Shell is a deliberate user choice with no
//! recovery path if that one password is lost. This is the standard
//! "recovery key" pattern full-disk-encryption tools use, deliberately
//! requiring both slots to hold real, independently-memorized passwords
//! (never a low-entropy PIN/pattern) — see PROJECT_MAP.md for the
//! reasoning the user and this project settled on.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::container;
use crate::error::{Result, VaultError};
use crate::files::FileVault;
use crate::kdf::{Argon2Params, VaultKey, SALT_LEN};
use crate::keyslots::KeySlots;
use crate::ledger::LedgerVault;
use crate::meta::meta_path_for;
use crate::now_rfc3339;
use crate::seeds::SeedVault;
use crate::store::Vault;

pub const PRIMARY_SLOT: &str = "primary";
pub const RECOVERY_SLOT: &str = "recovery";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS zones (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    kind           TEXT NOT NULL,
    label          TEXT NOT NULL,
    icon           TEXT NOT NULL,
    kdf_salt       BLOB NOT NULL,
    kdf_m_cost_kib INTEGER NOT NULL,
    kdf_t_cost     INTEGER NOT NULL,
    kdf_p_cost     INTEGER NOT NULL,
    blob           BLOB NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL
);
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoneKind {
    Accounts,
    Files,
    Seeds,
    Ledger,
}

impl ZoneKind {
    fn as_str(self) -> &'static str {
        match self {
            ZoneKind::Accounts => "accounts",
            ZoneKind::Files => "files",
            ZoneKind::Seeds => "seeds",
            ZoneKind::Ledger => "ledger",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "accounts" => Ok(ZoneKind::Accounts),
            "files" => Ok(ZoneKind::Files),
            "seeds" => Ok(ZoneKind::Seeds),
            "ledger" => Ok(ZoneKind::Ledger),
            other => Err(VaultError::InvalidMeta(format!(
                "unknown zone kind: {other}"
            ))),
        }
    }

    fn schema(self) -> &'static str {
        match self {
            ZoneKind::Accounts => crate::store::SCHEMA,
            ZoneKind::Files => crate::files::SCHEMA,
            ZoneKind::Seeds => crate::seeds::SCHEMA,
            ZoneKind::Ledger => crate::ledger::SCHEMA,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneMeta {
    pub id: i64,
    pub kind: String,
    pub label: String,
    pub icon: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A zone opened from the Shell: the live, unlocked, in-memory vault. Pass
/// it to `Shell::save_zone` after any mutation to persist the change.
pub enum OpenZone {
    Accounts(Vault),
    Files(FileVault),
    Seeds(SeedVault),
    Ledger(LedgerVault),
}

pub struct Shell {
    conn: Connection,
    db_path: PathBuf,
    #[allow(dead_code)]
    key: VaultKey,
}

impl std::fmt::Debug for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shell")
            .field("db_path", &self.db_path)
            .finish_non_exhaustive()
    }
}

impl Shell {
    /// Create a brand-new Shell protected by `primary_password`, and
    /// optionally a second, equally powerful `recovery_password` — when
    /// present, either password opens the Shell, and either can be used
    /// later to set a new value for the other via `change_password`. Both
    /// must be real passwords: there is deliberately no way to add a
    /// low-entropy PIN/pattern slot (see the module doc comment for why).
    ///
    /// With `recovery_password: None`, the Shell has exactly one password
    /// and no recovery path at all if it's forgotten — a deliberate
    /// "strict" mode the user can choose over the safer two-password
    /// default. A recovery slot can still be added *later* via
    /// `change_password(primary_password, RECOVERY_SLOT, ..., ...)`, since
    /// that call creates the slot if it doesn't already exist.
    pub fn create(
        db_path: impl AsRef<Path>,
        primary_password: &str,
        recovery_password: Option<&str>,
        params: Argon2Params,
    ) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        if db_path.exists() {
            return Err(VaultError::AlreadyExists(db_path));
        }
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let data_key = VaultKey::random();
        let mut slots = KeySlots::new();
        slots.set_slot(PRIMARY_SLOT, primary_password, params, &data_key)?;
        if let Some(recovery_password) = recovery_password {
            slots.set_slot(RECOVERY_SLOT, recovery_password, params, &data_key)?;
        }
        slots.save(&meta_path_for(&db_path))?;

        let conn = container::create_with_key(&db_path, &data_key, SCHEMA)?;
        Ok(Self {
            conn,
            db_path,
            key: data_key,
        })
    }

    /// Open with the **primary** password only. The recovery password (if
    /// one exists) deliberately does not work here — it only works through
    /// `change_password`'s recovery flow. This is a usability/hygiene
    /// choice, not an added cryptographic barrier: a recovery password
    /// can always reset the primary and then open normally anyway, so
    /// anyone who has it can still get in, just via one extra step. What
    /// this *does* buy: the recovery password is never typed into the
    /// everyday unlock screen, so it stays rarely-used and its "emergency
    /// only" purpose stays unambiguous — see PROJECT_MAP.md.
    pub fn open(db_path: impl AsRef<Path>, password: &str) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();
        if !db_path.exists() {
            return Err(VaultError::NotFound(db_path));
        }
        let meta_path = meta_path_for(&db_path);
        if !meta_path.exists() {
            return Err(VaultError::InvalidMeta(format!(
                "missing key-slot file: {}",
                meta_path.display()
            )));
        }
        let slots = KeySlots::load(&meta_path)?;
        let data_key = slots.unlock_one_of(password, &[PRIMARY_SLOT])?;

        let conn = container::open_with_key(&db_path, &data_key, SCHEMA)?;
        Ok(Self {
            conn,
            db_path,
            key: data_key,
        })
    }

    /// Set a new password for `slot_name` (`PRIMARY_SLOT` or
    /// `RECOVERY_SLOT`), authenticated by *any* currently-valid password
    /// (that slot's own, or the other one) — this is the one place the
    /// recovery password is actually usable. This is what makes it
    /// bidirectional: forgetting one password just means resetting it with
    /// the other, in either direction.
    ///
    /// Deliberately an associated function taking `db_path` directly
    /// rather than `&self` — it only ever touches the key-slot sidecar
    /// file, never the SQLCipher connection, so it must not require going
    /// through `Shell::open` (which only accepts the primary password) to
    /// get a `Shell` handle first. Recovering via the recovery password
    /// would otherwise be impossible: that's the exact scenario this
    /// exists for.
    pub fn change_password(
        db_path: impl AsRef<Path>,
        known_password: &str,
        slot_name: &str,
        new_password: &str,
        params: Argon2Params,
    ) -> Result<()> {
        let meta_path = meta_path_for(db_path.as_ref());
        let mut slots = KeySlots::load(&meta_path)?;
        let data_key = slots.unlock_any(known_password)?;
        slots.set_slot(slot_name, new_password, params, &data_key)?;
        slots.save(&meta_path)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Back up the whole Shell — every zone it contains, still encrypted —
    /// to another location, byte-for-byte, openable later with the same
    /// Shell password. This is the "3-2-1 backup" the spec requires, now a
    /// single action at the Shell level since individual zones no longer
    /// have their own files to copy.
    pub fn export_backup(&self, dest_dir: impl AsRef<Path>) -> Result<PathBuf> {
        let dest_dir = dest_dir.as_ref();
        std::fs::create_dir_all(dest_dir)?;
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;

        let file_name = self
            .db_path
            .file_name()
            .ok_or_else(|| VaultError::InvalidMeta("shell path has no file name".into()))?;
        let dest_db = dest_dir.join(file_name);
        std::fs::copy(&self.db_path, &dest_db)?;

        let src_meta = crate::meta::meta_path_for(&self.db_path);
        let dest_meta = crate::meta::meta_path_for(&dest_db);
        std::fs::copy(&src_meta, &dest_meta)?;

        Ok(dest_db)
    }

    pub fn list_zones(&self) -> Result<Vec<ZoneMeta>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, label, icon, created_at, updated_at FROM zones ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ZoneMeta {
                id: row.get(0)?,
                kind: row.get(1)?,
                label: row.get(2)?,
                icon: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Create a brand-new, empty zone of `kind`, protected by its own
    /// `zone_password` — completely independent of the Shell's password.
    pub fn create_zone(
        &self,
        kind: ZoneKind,
        label: &str,
        icon: &str,
        zone_password: &str,
        params: Argon2Params,
    ) -> Result<i64> {
        let (conn, key, salt) = container::create_in_memory(zone_password, params, kind.schema())?;
        let plaintext = container::serialize_to_bytes(&conn)?;
        let blob = container::encrypt_blob(&key, &plaintext);
        drop(conn);

        let now = now_rfc3339();
        self.conn.execute(
            "INSERT INTO zones
                (kind, label, icon, kdf_salt, kdf_m_cost_kib, kdf_t_cost, kdf_p_cost, blob, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            rusqlite::params![
                kind.as_str(),
                label,
                icon,
                salt.as_slice(),
                params.m_cost_kib,
                params.t_cost,
                params.p_cost,
                blob,
                now,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn rename_zone(&self, zone_id: i64, new_label: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE zones SET label = ?1 WHERE id = ?2",
            rusqlite::params![new_label, zone_id],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(zone_id));
        }
        Ok(())
    }

    pub fn set_zone_icon(&self, zone_id: i64, icon: &str) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE zones SET icon = ?1 WHERE id = ?2",
            rusqlite::params![icon, zone_id],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(zone_id));
        }
        Ok(())
    }

    pub fn delete_zone(&self, zone_id: i64) -> Result<()> {
        let changed = self.conn.execute(
            "DELETE FROM zones WHERE id = ?1",
            rusqlite::params![zone_id],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(zone_id));
        }
        Ok(())
    }

    /// Decrypt zone `zone_id` with `zone_password` and hand back its raw
    /// kind, connection, and key — shared by `open_zone` (which wraps the
    /// result into a typed `OpenZone`) and `export_zone_standalone` (which
    /// only needs the live connection to copy out of).
    fn open_zone_raw(
        &self,
        zone_id: i64,
        zone_password: &str,
    ) -> Result<(ZoneKind, Connection, VaultKey)> {
        let row: (String, Vec<u8>, u32, u32, u32, Vec<u8>) = self
            .conn
            .query_row(
                "SELECT kind, kdf_salt, kdf_m_cost_kib, kdf_t_cost, kdf_p_cost, blob
                 FROM zones WHERE id = ?1",
                rusqlite::params![zone_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .map_err(|_| VaultError::EntryNotFound(zone_id))?;
        let (kind_str, salt_bytes, m_cost_kib, t_cost, p_cost, blob) = row;

        let kind = ZoneKind::parse(&kind_str)?;
        let salt: [u8; SALT_LEN] = salt_bytes
            .try_into()
            .map_err(|_| VaultError::InvalidMeta("corrupt zone KDF salt".into()))?;
        let params = Argon2Params {
            m_cost_kib,
            t_cost,
            p_cost,
        };

        let (conn, key) = container::open_in_memory(&blob, zone_password, &salt, params)?;
        Ok((kind, conn, key))
    }

    /// Open a zone by id with its own password. Returns
    /// `VaultError::InvalidPassword` for a wrong password (indistinguishable
    /// from corruption, same as every other zone).
    pub fn open_zone(&self, zone_id: i64, zone_password: &str) -> Result<OpenZone> {
        let (kind, conn, key) = self.open_zone_raw(zone_id, zone_password)?;

        Ok(match kind {
            ZoneKind::Accounts => OpenZone::Accounts(Vault::wrap(conn, key)),
            ZoneKind::Files => OpenZone::Files(FileVault::wrap(conn, key)),
            ZoneKind::Seeds => OpenZone::Seeds(SeedVault::wrap(conn, key)),
            ZoneKind::Ledger => OpenZone::Ledger(LedgerVault::wrap(conn, key)),
        })
    }

    /// Opt-in safety valve: export zone `zone_id` (authenticated by its own
    /// `zone_password`) to a brand-new, standalone, SQLCipher-encrypted file
    /// at `dest_path`, independently openable later with that same
    /// password via the zone kind's own `open()` — e.g. `Vault::open` for
    /// an Accounts zone. This deliberately re-introduces the exact
    /// existence-on-disk exposure the Shell/zones architecture exists to
    /// avoid (see the module doc comment and PROJECT_MAP.md): it is an
    /// explicit, one-time, user-initiated action, never automatic, and the
    /// Shell itself and every other zone remain exactly as hidden as
    /// before. Returns the zone's kind (as its usual lowercase string, e.g.
    /// `"accounts"`) so the caller knows which type to reopen it as.
    pub fn export_zone_standalone(
        &self,
        zone_id: i64,
        zone_password: &str,
        dest_path: impl AsRef<Path>,
        params: Argon2Params,
    ) -> Result<&'static str> {
        let (kind, conn, _key) = self.open_zone_raw(zone_id, zone_password)?;
        container::export_in_memory_to_encrypted_file(
            &conn,
            dest_path.as_ref(),
            zone_password,
            params,
        )?;
        Ok(kind.as_str())
    }

    /// Persist a zone's current in-memory state back into the Shell — call
    /// this after any mutation made through the returned `OpenZone`.
    pub fn save_zone(&self, zone_id: i64, zone: &OpenZone) -> Result<()> {
        let (plaintext, key) = match zone {
            OpenZone::Accounts(v) => (v.serialize()?, v.key()),
            OpenZone::Files(f) => (f.serialize()?, f.key()),
            OpenZone::Seeds(s) => (s.serialize()?, s.key()),
            OpenZone::Ledger(l) => (l.serialize()?, l.key()),
        };
        let blob = container::encrypt_blob(key, &plaintext);
        let now = now_rfc3339();
        let changed = self.conn.execute(
            "UPDATE zones SET blob = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![blob, now, zone_id],
        )?;
        if changed == 0 {
            return Err(VaultError::EntryNotFound(zone_id));
        }
        Ok(())
    }
}
