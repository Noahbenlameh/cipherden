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
        id = vault
            .add_file(None, "photo.jpg", b"fake-jpeg-bytes")
            .unwrap();
    }

    let vault = FileVault::open(&db_path, "master-pw").unwrap();
    let files = vault.list_files(None).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "photo.jpg");
    assert_eq!(files[0].size, "fake-jpeg-bytes".len() as i64);
    assert_eq!(files[0].folder_id, None);

    let (meta, data) = vault.read_file(id).unwrap().unwrap();
    assert_eq!(meta.name, "photo.jpg");
    assert_eq!(data, b"fake-jpeg-bytes");
}

#[test]
fn list_files_does_not_load_blob_contents_into_meta() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();
    vault
        .add_file(None, "big.bin", &vec![0xAB; 10_000])
        .unwrap();

    let files = vault.list_files(None).unwrap();
    assert_eq!(files[0].size, 10_000);
}

#[test]
fn delete_file_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();
    let id = vault.add_file(None, "note.txt", b"hello").unwrap();

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
        vault.add_file(None, "a.txt", b"x").unwrap();
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
    vault.add_file(None, "doc.pdf", b"%PDF-fake").unwrap();
    let backup_path = vault.export_backup(&backup_dir).unwrap();

    let restored = FileVault::open(&backup_path, "pw").unwrap();
    let files = restored.list_files(None).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "doc.pdf");
}

#[test]
fn accounts_vault_and_file_vault_are_independent_containers() {
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
    files.add_file(None, "secret.txt", b"shh").unwrap();

    assert_eq!(accounts.list_entries().unwrap().len(), 1);
    assert_eq!(files.list_files(None).unwrap().len(), 1);

    let err = vault_core::Vault::open(dir.path().join("accounts.vault"), "files-pw").unwrap_err();
    assert!(matches!(err, VaultError::InvalidPassword));
}

// --- folders ---------------------------------------------------------------

#[test]
fn create_folder_and_move_file_into_it() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();

    let docs = vault.create_folder(None, "Documents").unwrap();
    let file_id = vault.add_file(None, "resume.pdf", b"pdf-bytes").unwrap();

    // Starts at root, not inside the folder.
    assert_eq!(vault.list_files(None).unwrap().len(), 1);
    assert_eq!(vault.list_files(Some(docs)).unwrap().len(), 0);

    vault.move_file(file_id, Some(docs)).unwrap();

    assert_eq!(vault.list_files(None).unwrap().len(), 0);
    let in_folder = vault.list_files(Some(docs)).unwrap();
    assert_eq!(in_folder.len(), 1);
    assert_eq!(in_folder[0].folder_id, Some(docs));
}

#[test]
fn nested_folders_and_listing_is_not_recursive() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();

    let parent = vault.create_folder(None, "Parent").unwrap();
    let child = vault.create_folder(Some(parent), "Child").unwrap();
    vault.add_file(Some(child), "deep.txt", b"x").unwrap();

    // Listing the parent folder shows only its direct subfolder, not the
    // grandchild file living two levels down.
    assert_eq!(vault.list_files(Some(parent)).unwrap().len(), 0);
    assert_eq!(vault.list_folders(Some(parent)).unwrap().len(), 1);
    assert_eq!(vault.list_files(Some(child)).unwrap().len(), 1);
}

#[test]
fn delete_folder_refuses_when_not_empty() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();

    let folder = vault.create_folder(None, "Stuff").unwrap();
    vault.add_file(Some(folder), "a.txt", b"x").unwrap();

    let err = vault.delete_folder(folder).unwrap_err();
    assert!(matches!(err, VaultError::FolderNotEmpty(_)));

    // Empty it, then deletion succeeds.
    let file_id = vault.list_files(Some(folder)).unwrap()[0].id;
    vault.delete_file(file_id).unwrap();
    vault.delete_folder(folder).unwrap();
    assert!(vault.list_folders(None).unwrap().is_empty());
}

#[test]
fn move_folder_refuses_cycles() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();

    let parent = vault.create_folder(None, "Parent").unwrap();
    let child = vault.create_folder(Some(parent), "Child").unwrap();

    // Can't move a folder into itself...
    let err = vault.move_folder(parent, Some(parent)).unwrap_err();
    assert!(matches!(err, VaultError::CyclicFolderMove));

    // ...or into its own descendant.
    let err = vault.move_folder(parent, Some(child)).unwrap_err();
    assert!(matches!(err, VaultError::CyclicFolderMove));

    // A legitimate move still works.
    let other = vault.create_folder(None, "Other").unwrap();
    vault.move_folder(child, Some(other)).unwrap();
    assert_eq!(vault.list_folders(Some(other)).unwrap().len(), 1);
}

#[test]
fn pinned_items_sort_first() {
    let dir = tempfile::tempdir().unwrap();
    let vault = FileVault::create(dir.path().join("files.vault"), "pw", test_params()).unwrap();

    vault.add_file(None, "a-normal.txt", b"x").unwrap();
    let pinned_id = vault.add_file(None, "z-pinned.txt", b"x").unwrap();
    vault.set_file_pinned(pinned_id, true).unwrap();

    let files = vault.list_files(None).unwrap();
    assert_eq!(files[0].name, "z-pinned.txt");
    assert!(files[0].pinned);
    assert!(!files[1].pinned);
}
