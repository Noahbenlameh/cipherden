use vault_core::export::export_csv;
use vault_core::kdf::Argon2Params;
use vault_core::store::NewEntry;
use vault_core::Vault;

fn test_params() -> Argon2Params {
    Argon2Params::for_testing()
}

#[test]
fn export_csv_writes_only_selected_entries() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path().join("vault.db"), "master-pw", test_params()).unwrap();

    let id1 = vault
        .add_entry(&NewEntry {
            title: "GitHub".into(),
            username: "alice".into(),
            password: "pw1".into(),
            url: "https://github.com".into(),
            notes: "".into(),
            category: "dev".into(),
        })
        .unwrap();
    let id2 = vault
        .add_entry(&NewEntry {
            title: "Bank".into(),
            username: "bob".into(),
            password: "pw2".into(),
            url: "".into(),
            notes: "".into(),
            category: "finance".into(),
        })
        .unwrap();

    let out_path = dir.path().join("export.csv");
    let written = export_csv(&vault, &[id1], &out_path).unwrap();
    assert_eq!(written, 1);

    let contents = std::fs::read_to_string(&out_path).unwrap();
    assert!(contents.contains("GitHub"));
    assert!(!contents.contains("Bank"));

    let _ = id2; // only asserting it was excluded, above
}

#[test]
fn export_csv_round_trips_through_import() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path().join("vault.db"), "master-pw", test_params()).unwrap();

    let id = vault
        .add_entry(&NewEntry {
            title: "Email".into(),
            username: "me@example.com".into(),
            password: "secret".into(),
            url: "https://mail.example.com".into(),
            notes: "personal".into(),
            category: "personal".into(),
        })
        .unwrap();

    let csv_path = dir.path().join("export.csv");
    export_csv(&vault, &[id], &csv_path).unwrap();

    let vault2 = Vault::create(dir.path().join("vault2.db"), "master-pw-2", test_params()).unwrap();
    let imported = vault_core::import::import_csv(&vault2, &csv_path).unwrap();
    assert_eq!(imported, 1);

    let entries = vault2.list_entries().unwrap();
    assert_eq!(entries[0].title, "Email");
    assert_eq!(entries[0].password, "secret");
}

#[test]
fn export_csv_skips_unknown_ids_without_failing() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path().join("vault.db"), "master-pw", test_params()).unwrap();
    let out_path = dir.path().join("export.csv");

    let written = export_csv(&vault, &[9999], &out_path).unwrap();
    assert_eq!(written, 0);
}
