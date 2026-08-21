use std::io::Write;

use keepass::{Database, DatabaseKey};
use vault_core::import::{import_csv, import_kdbx};
use vault_core::kdf::Argon2Params;
use vault_core::Vault;

fn test_params() -> Argon2Params {
    Argon2Params::for_testing()
}

#[test]
fn import_csv_maps_common_headers_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");
    let vault = Vault::create(&db_path, "master-pw", test_params()).unwrap();

    let csv_path = dir.path().join("export.csv");
    let mut f = std::fs::File::create(&csv_path).unwrap();
    writeln!(f, "Title,Username,Password,URL,Notes,Category").unwrap();
    writeln!(
        f,
        "GitHub,alice,hunter2,https://github.com,work account,dev"
    )
    .unwrap();
    writeln!(f, "Bank,bob,swordfish,https://bank.example,,finance").unwrap();
    drop(f);

    let imported = import_csv(&vault, &csv_path).unwrap();
    assert_eq!(imported, 2);

    let entries = vault.list_entries().unwrap();
    assert_eq!(entries.len(), 2);
    let github = entries.iter().find(|e| e.title == "GitHub").unwrap();
    assert_eq!(github.username, "alice");
    assert_eq!(github.password, "hunter2");
    assert_eq!(github.category, "dev");
}

#[test]
fn import_csv_falls_back_to_url_when_title_missing() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");
    let vault = Vault::create(&db_path, "master-pw", test_params()).unwrap();

    let csv_path = dir.path().join("export.csv");
    let mut f = std::fs::File::create(&csv_path).unwrap();
    writeln!(f, "username,password,url").unwrap();
    writeln!(f, "carol,pw123,https://example.com").unwrap();
    drop(f);

    let imported = import_csv(&vault, &csv_path).unwrap();
    assert_eq!(imported, 1);

    let entries = vault.list_entries().unwrap();
    assert_eq!(entries[0].title, "https://example.com");
    assert_eq!(entries[0].username, "carol");
}

#[test]
fn import_kdbx_recurses_into_groups_and_uses_them_as_category() {
    let dir = tempfile::tempdir().unwrap();

    // Build a small KeePass database in memory and save it to disk, so this
    // test has no dependency on an external fixture file.
    let mut kdbx_db = Database::new();
    {
        let mut root = kdbx_db.root_mut();
        let mut entry = root.add_entry();
        entry.set_unprotected("Title", "Root Entry");
        entry.set_unprotected("UserName", "root-user");
        entry.set_protected("Password", "root-pass");
        entry.set_unprotected("URL", "https://root.example");

        let mut group = root.add_group();
        group.name = "Work".to_string();
        let mut nested = group.add_entry();
        nested.set_unprotected("Title", "Nested Entry");
        nested.set_unprotected("UserName", "nested-user");
        nested.set_protected("Password", "nested-pass");
    }

    let kdbx_path = dir.path().join("import.kdbx");
    let mut out = std::fs::File::create(&kdbx_path).unwrap();
    kdbx_db
        .save(&mut out, DatabaseKey::new().with_password("kdbx-pw"))
        .unwrap();
    drop(out);

    let vault_path = dir.path().join("vault.db");
    let vault = Vault::create(&vault_path, "master-pw", test_params()).unwrap();

    let imported = import_kdbx(&vault, &kdbx_path, "kdbx-pw").unwrap();
    assert_eq!(imported, 2);

    let entries = vault.list_entries().unwrap();
    let root_entry = entries.iter().find(|e| e.title == "Root Entry").unwrap();
    assert_eq!(root_entry.username, "root-user");
    assert_eq!(root_entry.password, "root-pass");
    assert_eq!(root_entry.category, "");

    let nested_entry = entries.iter().find(|e| e.title == "Nested Entry").unwrap();
    assert_eq!(nested_entry.username, "nested-user");
    assert_eq!(nested_entry.category, "Work");
}

#[test]
fn import_kdbx_rejects_wrong_password() {
    let dir = tempfile::tempdir().unwrap();

    let mut kdbx_db = Database::new();
    {
        let mut root = kdbx_db.root_mut();
        let mut entry = root.add_entry();
        entry.set_unprotected("Title", "Something");
    }
    let kdbx_path = dir.path().join("import.kdbx");
    let mut out = std::fs::File::create(&kdbx_path).unwrap();
    kdbx_db
        .save(&mut out, DatabaseKey::new().with_password("correct-pw"))
        .unwrap();
    drop(out);

    let vault_path = dir.path().join("vault.db");
    let vault = Vault::create(&vault_path, "master-pw", test_params()).unwrap();

    let err = import_kdbx(&vault, &kdbx_path, "wrong-pw").unwrap_err();
    assert!(matches!(err, vault_core::VaultError::InvalidMeta(_)));
}
