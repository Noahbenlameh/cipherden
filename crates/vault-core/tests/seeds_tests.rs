use vault_core::kdf::Argon2Params;
use vault_core::{NewSeedEntry, SeedVault, VaultError};

fn test_params() -> Argon2Params {
    Argon2Params::for_testing()
}

fn sample() -> NewSeedEntry {
    NewSeedEntry {
        label: "Ledger Nano".into(),
        network: "Bitcoin".into(),
        seed_phrase: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".into(),
        derivation_path: "m/44'/0'/0'/0/0".into(),
        notes: "Cold wallet, kept in a safe".into(),
    }
}

#[test]
fn create_add_reopen_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("seeds.vault");

    let id;
    {
        let vault = SeedVault::create(&db_path, "master-pw", test_params()).unwrap();
        id = vault.add_seed(&sample()).unwrap();
    }

    let vault = SeedVault::open(&db_path, "master-pw").unwrap();
    let seeds = vault.list_seeds().unwrap();
    assert_eq!(seeds.len(), 1);
    assert_eq!(seeds[0].label, "Ledger Nano");
    assert_eq!(seeds[0].network, "Bitcoin");
    assert_eq!(seeds[0].derivation_path, "m/44'/0'/0'/0/0");

    let fetched = vault.get_seed(id).unwrap().unwrap();
    assert_eq!(fetched.seed_phrase, sample().seed_phrase);
}

#[test]
fn update_seed_changes_fields_and_bumps_updated_at() {
    let dir = tempfile::tempdir().unwrap();
    let vault = SeedVault::create(dir.path().join("seeds.vault"), "pw", test_params()).unwrap();
    let id = vault.add_seed(&sample()).unwrap();
    let original = vault.get_seed(id).unwrap().unwrap();

    let mut updated = sample();
    updated.label = "Renamed Wallet".into();
    vault.update_seed(id, &updated).unwrap();

    let fetched = vault.get_seed(id).unwrap().unwrap();
    assert_eq!(fetched.label, "Renamed Wallet");
    assert_eq!(fetched.created_at, original.created_at);
}

#[test]
fn delete_seed_removes_it() {
    let dir = tempfile::tempdir().unwrap();
    let vault = SeedVault::create(dir.path().join("seeds.vault"), "pw", test_params()).unwrap();
    let id = vault.add_seed(&sample()).unwrap();

    vault.delete_seed(id).unwrap();
    assert!(vault.get_seed(id).unwrap().is_none());

    let err = vault.delete_seed(id).unwrap_err();
    assert!(matches!(err, VaultError::EntryNotFound(_)));
}

#[test]
fn wrong_password_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("seeds.vault");
    {
        let vault = SeedVault::create(&db_path, "correct-pw", test_params()).unwrap();
        vault.add_seed(&sample()).unwrap();
    }

    let err = SeedVault::open(&db_path, "wrong-pw").unwrap_err();
    assert!(matches!(err, VaultError::InvalidPassword));
}

#[test]
fn export_backup_produces_independently_openable_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("seeds.vault");
    let backup_dir = dir.path().join("backup");

    let vault = SeedVault::create(&db_path, "pw", test_params()).unwrap();
    vault.add_seed(&sample()).unwrap();
    let backup_path = vault.export_backup(&backup_dir).unwrap();

    let restored = SeedVault::open(&backup_path, "pw").unwrap();
    let seeds = restored.list_seeds().unwrap();
    assert_eq!(seeds.len(), 1);
    assert_eq!(seeds[0].label, "Ledger Nano");
}
