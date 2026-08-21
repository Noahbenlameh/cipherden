use vault_core::kdf::Argon2Params;
use vault_core::store::NewEntry;
use vault_core::{Vault, VaultError};

fn test_params() -> Argon2Params {
    Argon2Params::for_testing()
}

fn sample_entry() -> NewEntry {
    NewEntry {
        title: "GitHub".into(),
        username: "alice".into(),
        password: "correct horse battery staple".into(),
        url: "https://github.com".into(),
        notes: "personal account".into(),
        category: "dev".into(),
    }
}

#[test]
fn create_add_reopen_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");

    {
        let vault = Vault::create(&db_path, "hunter2-master", test_params()).unwrap();
        let id = vault.add_entry(&sample_entry()).unwrap();
        assert!(id > 0);
    } // vault dropped, connection closed

    let vault = Vault::open(&db_path, "hunter2-master").unwrap();
    let entries = vault.list_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "GitHub");
    assert_eq!(entries[0].username, "alice");
    assert_eq!(entries[0].password, "correct horse battery staple");
}

#[test]
fn wrong_password_is_rejected_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");

    {
        let vault = Vault::create(&db_path, "correct-password", test_params()).unwrap();
        vault.add_entry(&sample_entry()).unwrap();
    }

    let err = Vault::open(&db_path, "totally-wrong-password").unwrap_err();
    assert!(matches!(err, VaultError::InvalidPassword));
}

#[test]
fn create_refuses_to_overwrite_existing_vault() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");

    Vault::create(&db_path, "pw1", test_params()).unwrap();
    let err = Vault::create(&db_path, "pw2", test_params()).unwrap_err();
    assert!(matches!(err, VaultError::AlreadyExists(_)));
}

#[test]
fn create_makes_missing_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir
        .path()
        .join("nested")
        .join("does")
        .join("not")
        .join("exist")
        .join("vault.db");

    Vault::create(&db_path, "pw", test_params()).unwrap();
    assert!(db_path.exists());
}

#[test]
fn open_missing_vault_reports_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("does-not-exist.db");

    let err = Vault::open(&db_path, "whatever").unwrap_err();
    assert!(matches!(err, VaultError::NotFound(_)));
}

#[test]
fn crud_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");
    let vault = Vault::create(&db_path, "master-pw", test_params()).unwrap();

    let id = vault.add_entry(&sample_entry()).unwrap();

    let mut updated = sample_entry();
    updated.password = "new-rotated-password".into();
    vault.update_entry(id, &updated).unwrap();

    let fetched = vault.get_entry(id).unwrap().unwrap();
    assert_eq!(fetched.password, "new-rotated-password");
    assert_ne!(fetched.created_at, ""); // timestamps populated

    vault.delete_entry(id).unwrap();
    assert!(vault.get_entry(id).unwrap().is_none());

    let err = vault.delete_entry(id).unwrap_err();
    assert!(matches!(err, VaultError::EntryNotFound(_)));
}

#[test]
fn search_is_case_insensitive_and_scoped_to_relevant_fields() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");
    let vault = Vault::create(&db_path, "master-pw", test_params()).unwrap();

    vault.add_entry(&sample_entry()).unwrap();
    vault
        .add_entry(&NewEntry {
            title: "Bank".into(),
            username: "bob".into(),
            password: "x".into(),
            url: "https://bank.example".into(),
            notes: "".into(),
            category: "finance".into(),
        })
        .unwrap();

    let results = vault.search("github").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "GitHub");

    let results = vault.search("finance").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Bank");

    let results = vault.search("nonexistent").unwrap();
    assert!(results.is_empty());
}

#[test]
fn tampering_with_db_bytes_is_detected_not_silently_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");

    {
        let vault = Vault::create(&db_path, "master-pw", test_params()).unwrap();
        vault.add_entry(&sample_entry()).unwrap();
    }

    // Flip bytes past the plaintext SQLite header region to corrupt ciphertext.
    let mut bytes = std::fs::read(&db_path).unwrap();
    assert!(bytes.len() > 200, "expected a non-trivial db file");
    for b in bytes.iter_mut().skip(150).take(32) {
        *b ^= 0xFF;
    }
    std::fs::write(&db_path, bytes).unwrap();

    // The corrupted bytes fall within SQLCipher's first page (the schema
    // page), which is exactly what our key-verification query reads, so
    // SQLCipher's per-page HMAC must cause this to fail rather than
    // silently hand back garbage as if it were valid plaintext.
    let err = Vault::open(&db_path, "master-pw").unwrap_err();
    assert!(matches!(err, VaultError::InvalidPassword));
}

#[test]
fn export_backup_produces_independently_openable_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vault.db");
    let backup_dir = dir.path().join("backup");

    let vault = Vault::create(&db_path, "master-pw", test_params()).unwrap();
    vault.add_entry(&sample_entry()).unwrap();
    let backup_path = vault.export_backup(&backup_dir).unwrap();

    assert!(backup_path.exists());

    let restored = Vault::open(&backup_path, "master-pw").unwrap();
    let entries = restored.list_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "GitHub");
}
