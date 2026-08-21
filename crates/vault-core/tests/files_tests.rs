use vault_core::kdf::Argon2Params;
use vault_core::{FileVault, VaultError};

fn test_params() -> Argon2Params {
    Argon2Params::for_testing()
}

#[test]
fn create_add_reopen_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("files.vault");

    let id;
    {
        let vault = FileVault::create(&db_path, "master-pw", test_params()).unwrap();
        id = vault.add_file("photo.jpg", b"fake-jpeg-bytes").unwrap();
    }

    let vault = FileVault::open(&db_path, "master-pw").unwrap();
    let files = vault.list_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "photo.jpg");
    assert_eq!(files[0].size, "fake-jpeg-bytes".len() as i64);

    let (meta, data) = vault.read_file(id).unwrap().unwrap();
    assert_eq!(meta.name, "photo.jpg");
    assert_eq!(data, b"fake-jpeg-bytes");
}

#[test]
fn list_files_does_not_load_blob_contents_into_meta() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();
    vault.add_file("big.bin", &vec![0xAB; 10_000]).unwrap();

    // FileMeta simply has no field for the blob — this test exists to pin
    // that down as an intentional API shape, not an oversight.
    let files = vault.list_files().unwrap();
    assert_eq!(files[0].size, 10_000);
}

#[test]
fn delete_file_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();
    let id = vault.add_file("note.txt", b"hello").unwrap();

    vault.delete_file(id).unwrap();
    assert!(vault.read_file(id).unwrap().is_none());

    let err = vault.delete_file(id).unwrap_err();
    assert!(matches!(err, VaultError::EntryNotFound(_)));
}

#[test]
fn wrong_password_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("files.vault");
    {
        let vault = FileVault::create(&db_path, "correct-pw", test_params()).unwrap();
        vault.add_file("a.txt", b"x").unwrap();
    }

    let err = FileVault::open(&db_path, "wrong-pw").unwrap_err();
    assert!(matches!(err, VaultError::InvalidPassword));
}

#[test]
fn export_backup_produces_independently_openable_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("files.vault");
    let backup_dir = dir.path().join("backup");

    let vault = FileVault::create(&db_path, "pw", test_params()).unwrap();
    vault.add_file("doc.pdf", b"%PDF-fake").unwrap();
    let backup_path = vault.export_backup(&backup_dir).unwrap();

    let restored = FileVault::open(&backup_path, "pw").unwrap();
    let files = restored.list_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "doc.pdf");
}

#[test]
fn accounts_vault_and_file_vault_are_independent_containers() {
    // Same directory, two different zones, two different passwords — they
    // must not interfere with each other at all.
    let dir = tempfile::tempdir().unwrap();

    let accounts =
        vault_core::Vault::create(dir.path().join("accounts.vault"), "acct-pw", test_params())
            .unwrap();
    accounts
        .add_entry(&vault_core::store::NewEntry {
            title: "GitHub".into(),
            ..Default::default()
        })
        .unwrap();

    let files =
        FileVault::create(dir.path().join("files.vault"), "files-pw", test_params()).unwrap();
    files.add_file("secret.txt", b"shh").unwrap();

    assert_eq!(accounts.list_entries().unwrap().len(), 1);
    assert_eq!(files.list_files().unwrap().len(), 1);

    // Wrong zone's password must not open the other zone.
    let err = vault_core::Vault::open(dir.path().join("accounts.vault"), "files-pw").unwrap_err();
    assert!(matches!(err, VaultError::InvalidPassword));
}
