//! Exporting entries back out to CSV — the mirror of `import::import_csv`,
//! using the same column layout so a round-trip (export, edit in a
//! spreadsheet, re-import) works without translation.

use std::path::Path;

use crate::error::Result;
use crate::store::Vault;

/// Write the given entries (by id) to a CSV file at `dest`. Unknown ids are
/// silently skipped (the caller's selection may be stale by a race with
/// another edit); returns how many rows were actually written.
pub fn export_csv(vault: &Vault, ids: &[i64], dest: impl AsRef<Path>) -> Result<usize> {
    let mut writer = csv::WriterBuilder::new()
        .from_path(dest.as_ref())
        .map_err(csv_err)?;

    writer
        .write_record(["Title", "Username", "Password", "URL", "Notes", "Category"])
        .map_err(csv_err)?;

    let mut count = 0;
    for &id in ids {
        let Some(entry) = vault.get_entry(id)? else {
            continue;
        };
        writer
            .write_record([
                &entry.title,
                &entry.username,
                &entry.password,
                &entry.url,
                &entry.notes,
                &entry.category,
            ])
            .map_err(csv_err)?;
        count += 1;
    }

    writer.flush()?;
    Ok(count)
}

fn csv_err(e: csv::Error) -> crate::error::VaultError {
    crate::error::VaultError::InvalidMeta(format!("CSV export error: {e}"))
}
