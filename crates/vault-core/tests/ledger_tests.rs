use vault_core::kdf::Argon2Params;
use vault_core::{LedgerVault, VaultError};

fn test_params() -> Argon2Params {
    Argon2Params::for_testing()
}

#[test]
fn create_add_reopen_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("ledger.vault");

    {
        let vault = LedgerVault::create(&db_path, "master-pw", test_params()).unwrap();
        vault.add_transaction(70_096, "Salary", None).unwrap();
        vault.add_transaction(-50_022, "Rent", None).unwrap();
    }

    let vault = LedgerVault::open(&db_path, "master-pw").unwrap();
    let txs = vault.list_transactions().unwrap();
    assert_eq!(txs.len(), 2);
    // Newest first.
    assert_eq!(txs[0].comment, "Rent");
    assert_eq!(txs[0].amount_cents, -50_022);
    assert_eq!(txs[1].comment, "Salary");
    assert!(!txs[0].date.is_empty());
}

#[test]
fn add_transaction_honors_an_explicit_date() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LedgerVault::create(dir.path().join("ledger.vault"), "pw", test_params()).unwrap();

    vault.add_transaction(1_000, "old paper record", Some("2019-03-14")).unwrap();
    vault.add_transaction(2_000, "today", None).unwrap();

    let txs = vault.list_transactions().unwrap();
    // Newest first: the undated (auto "now") entry comes first.
    assert_eq!(txs[0].comment, "today");
    assert_ne!(txs[0].date, "2019-03-14");
    assert_eq!(txs[1].comment, "old paper record");
    assert_eq!(txs[1].date, "2019-03-14");
}

#[test]
fn total_sums_all_amounts_without_float_drift() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LedgerVault::create(dir.path().join("ledger.vault"), "pw", test_params()).unwrap();

    assert_eq!(vault.total_cents().unwrap(), 0);

    vault.add_transaction(70_096, "in", None).unwrap();
    vault.add_transaction(-50_022, "out", None).unwrap();
    vault.add_transaction(1, "one cent", None).unwrap();

    assert_eq!(vault.total_cents().unwrap(), 70_096 - 50_022 + 1);
}

#[test]
fn update_transaction_keeps_original_date() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LedgerVault::create(dir.path().join("ledger.vault"), "pw", test_params()).unwrap();
    let id = vault.add_transaction(1_000, "typo", None).unwrap();
    let original_date = vault.list_transactions().unwrap()[0].date.clone();

    vault.update_transaction(id, 2_000, "fixed").unwrap();

    let txs = vault.list_transactions().unwrap();
    assert_eq!(txs[0].amount_cents, 2_000);
    assert_eq!(txs[0].comment, "fixed");
    assert_eq!(txs[0].date, original_date);
    assert_eq!(vault.total_cents().unwrap(), 2_000);
}

#[test]
fn delete_transaction_removes_it_and_updates_total() {
    let dir = tempfile::tempdir().unwrap();
    let vault = LedgerVault::create(dir.path().join("ledger.vault"), "pw", test_params()).unwrap();
    let id = vault.add_transaction(500, "temp", None).unwrap();
    vault.add_transaction(100, "keep", None).unwrap();

    vault.delete_transaction(id).unwrap();
    let txs = vault.list_transactions().unwrap();
    assert_eq!(txs.len(), 1);
    assert_eq!(txs[0].comment, "keep");
    assert_eq!(vault.total_cents().unwrap(), 100);

    let err = vault.delete_transaction(id).unwrap_err();
    assert!(matches!(err, VaultError::EntryNotFound(_)));
}

#[test]
fn wrong_password_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("ledger.vault");
    {
        let vault = LedgerVault::create(&db_path, "correct-pw", test_params()).unwrap();
        vault.add_transaction(100, "x", None).unwrap();
    }

    let err = LedgerVault::open(&db_path, "wrong-pw").unwrap_err();
    assert!(matches!(err, VaultError::InvalidPassword));
}

#[test]
fn export_backup_produces_independently_openable_copy() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("ledger.vault");
    let backup_dir = dir.path().join("backup");

    let vault = LedgerVault::create(&db_path, "pw", test_params()).unwrap();
    vault.add_transaction(12_345, "test", None).unwrap();
    let backup_path = vault.export_backup(&backup_dir).unwrap();

    let restored = LedgerVault::open(&backup_path, "pw").unwrap();
    assert_eq!(restored.total_cents().unwrap(), 12_345);
}
