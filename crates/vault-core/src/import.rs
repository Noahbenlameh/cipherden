//! Importing entries from other formats, per spec §4.2 (v0.2).
//!
//! Both importers only ever construct [`NewEntry`] values and hand them to
//! the same `Vault::add_entry` path normal CRUD uses — nothing here touches
//! the vault's own encryption. CSV parsing and KDBX decryption are both done
//! by vetted external crates (`csv`, `keepass`), not hand-rolled.

use std::fs::File;
use std::path::Path;

use keepass::db::GroupRef;
use keepass::{Database, DatabaseKey};

use crate::error::{Result, VaultError};
use crate::store::{NewEntry, Vault};

/// Import entries from a CSV file (e.g. an export of the Google Sheet this
/// project is meant to replace). Recognizes common header names
/// case-insensitively; any column not recognized is ignored. A row with no
/// usable title falls back to the URL, then the username, as the title so
/// nothing silently vanishes.
pub fn import_csv(vault: &Vault, path: impl AsRef<Path>) -> Result<usize> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(path.as_ref())
        .map_err(csv_err)?;

    let headers = reader.headers().map_err(csv_err)?.clone();
    let column_of = |names: &[&str]| -> Option<usize> {
        headers.iter().position(|h| {
            let h = h.trim().to_lowercase();
            names.iter().any(|n| *n == h)
        })
    };

    let col_title = column_of(&["title", "name"]);
    let col_username = column_of(&["username", "user", "login", "email"]);
    let col_password = column_of(&["password", "pass"]);
    let col_url = column_of(&["url", "website", "site"]);
    let col_notes = column_of(&["notes", "note", "comment"]);
    let col_category = column_of(&["category", "group", "folder"]);

    let get = |record: &csv::StringRecord, col: Option<usize>| -> String {
        col.and_then(|i| record.get(i))
            .unwrap_or("")
            .trim()
            .to_string()
    };

    let mut count = 0;
    for result in reader.records() {
        let record = result.map_err(csv_err)?;

        let username = get(&record, col_username);
        let url = get(&record, col_url);
        let mut title = get(&record, col_title);
        if title.is_empty() {
            title = if !url.is_empty() {
                url.clone()
            } else if !username.is_empty() {
                username.clone()
            } else {
                continue; // nothing identifiable about this row at all
            };
        }

        let entry = NewEntry {
            title,
            username,
            password: get(&record, col_password),
            url,
            notes: get(&record, col_notes),
            category: get(&record, col_category),
        };
        vault.add_entry(&entry)?;
        count += 1;
    }

    Ok(count)
}

fn csv_err(e: csv::Error) -> VaultError {
    VaultError::InvalidMeta(format!("CSV import error: {e}"))
}

/// Import every entry from a KeePass `.kdbx` file, recursively across all
/// groups. `kdbx_password` unlocks the *source* KeePass database — it has
/// nothing to do with this vault's own master password. Group names become
/// the category (nested groups are joined with "/").
pub fn import_kdbx(vault: &Vault, path: impl AsRef<Path>, kdbx_password: &str) -> Result<usize> {
    let mut file = File::open(path.as_ref())?;
    let key = DatabaseKey::new().with_password(kdbx_password);
    let db = Database::open(&mut file, key)
        .map_err(|e| VaultError::InvalidMeta(format!("KDBX import error: {e}")))?;

    let mut count = 0;
    import_group(vault, db.root(), "", &mut count)?;
    Ok(count)
}

fn import_group(
    vault: &Vault,
    group: GroupRef<'_>,
    path_prefix: &str,
    count: &mut usize,
) -> Result<()> {
    for e in group.entries() {
        let title = e.get_title().unwrap_or_default().to_string();
        let username = e.get_username().unwrap_or_default().to_string();
        if title.is_empty() && username.is_empty() {
            continue; // skip fully-empty template entries
        }
        let entry = NewEntry {
            title: if title.is_empty() {
                username.clone()
            } else {
                title
            },
            username,
            password: e.get_password().unwrap_or_default().to_string(),
            url: e.get_url().unwrap_or_default().to_string(),
            notes: e.get("Notes").unwrap_or_default().to_string(),
            category: path_prefix.to_string(),
        };
        vault.add_entry(&entry)?;
        *count += 1;
    }

    for g in group.groups() {
        let sub_path = if path_prefix.is_empty() {
            g.name.clone()
        } else {
            format!("{path_prefix}/{}", g.name)
        };
        import_group(vault, g, &sub_path, count)?;
    }

    Ok(())
}
