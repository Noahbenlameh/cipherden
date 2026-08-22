use vault_core::kdf::Argon2Params;
use vault_core::store::NewEntry;
use vault_core::{
    NewSeedEntry, OpenZone, Shell, VaultError, ZoneKind, PRIMARY_SLOT, RECOVERY_SLOT,
};

fn test_params() -> Argon2Params {
    Argon2Params::for_testing()
}

/// Regression test: `Shell::create` used to write the key-slot sidecar
/// file *before* creating the shell path's parent directory, so creating a
/// Shell at a not-yet-existing default path (e.g. `./vault/shell.vault`)
/// failed with "No such file or directory" the very first time.
#[test]
fn create_makes_missing_parent_directories() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir
        .path()
        .join("nested")
        .join("does")
        .join("not")
        .join("exist")
        .join("shell.vault");

    Shell::create(&shell_path, "pw", Some("recovery-pw"), test_params()).unwrap();
    assert!(shell_path.exists());

    let meta_path = shell_path.with_file_name(format!(
        "{}.meta.json",
        shell_path.file_name().unwrap().to_string_lossy()
    ));
    assert!(meta_path.exists());
}

#[test]
fn create_zone_open_mutate_save_reopen_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(
        &shell_path,
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();

    let zone_id = shell
        .create_zone(
            ZoneKind::Accounts,
            "My Accounts",
            "🔑",
            "zone-pw",
            test_params(),
        )
        .unwrap();

    {
        let OpenZone::Accounts(vault) = shell.open_zone(zone_id, "zone-pw").unwrap() else {
            panic!("expected Accounts zone");
        };
        vault
            .add_entry(&NewEntry {
                title: "GitHub".into(),
                username: "alice".into(),
                password: "hunter2".into(),
                ..Default::default()
            })
            .unwrap();
        shell
            .save_zone(zone_id, &OpenZone::Accounts(vault))
            .unwrap();
    }

    // Reopen the shell itself (simulating an app restart) and the zone.
    drop(shell);
    let shell = Shell::open(&shell_path, "shell-pw").unwrap();
    let OpenZone::Accounts(vault) = shell.open_zone(zone_id, "zone-pw").unwrap() else {
        panic!("expected Accounts zone");
    };
    let entries = vault.list_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "GitHub");
    assert_eq!(entries[0].password, "hunter2");
}

#[test]
fn change_zone_password_preserves_data_and_rejects_the_old_password() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(&shell_path, "shell-pw", None, test_params()).unwrap();

    let zone_id = shell
        .create_zone(
            ZoneKind::Accounts,
            "My Accounts",
            "🔑",
            "old-zone-pw",
            test_params(),
        )
        .unwrap();
    {
        let OpenZone::Accounts(vault) = shell.open_zone(zone_id, "old-zone-pw").unwrap() else {
            panic!("expected Accounts zone");
        };
        vault
            .add_entry(&NewEntry {
                title: "GitHub".into(),
                username: "alice".into(),
                password: "hunter2".into(),
                ..Default::default()
            })
            .unwrap();
        shell
            .save_zone(zone_id, &OpenZone::Accounts(vault))
            .unwrap();
    }

    shell
        .change_zone_password(zone_id, "old-zone-pw", "new-zone-pw", test_params())
        .unwrap();

    // Old password no longer works.
    match shell.open_zone(zone_id, "old-zone-pw") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected the old zone password to be rejected"),
    }

    // New password opens it, and the data is untouched.
    let OpenZone::Accounts(vault) = shell.open_zone(zone_id, "new-zone-pw").unwrap() else {
        panic!("expected Accounts zone");
    };
    let entries = vault.list_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "GitHub");
    assert_eq!(entries[0].password, "hunter2");
}

#[test]
fn change_zone_password_requires_the_correct_old_password() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(&shell_path, "shell-pw", None, test_params()).unwrap();
    let zone_id = shell
        .create_zone(
            ZoneKind::Accounts,
            "My Accounts",
            "🔑",
            "real-pw",
            test_params(),
        )
        .unwrap();

    match shell.change_zone_password(zone_id, "wrong-pw", "new-pw", test_params()) {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword"),
    }

    // The zone is unaffected -- the real password still works.
    assert!(shell.open_zone(zone_id, "real-pw").is_ok());
}

#[test]
fn files_zone_round_trips_through_shell() {
    let dir = tempfile::tempdir().unwrap();
    let shell = Shell::create(
        dir.path().join("shell.vault"),
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();

    let zone_id = shell
        .create_zone(ZoneKind::Files, "My Files", "🗂", "files-pw", test_params())
        .unwrap();

    let OpenZone::Files(vault) = shell.open_zone(zone_id, "files-pw").unwrap() else {
        panic!("expected Files zone");
    };
    vault.add_file(None, "note.txt", b"hello").unwrap();
    shell.save_zone(zone_id, &OpenZone::Files(vault)).unwrap();

    let OpenZone::Files(vault) = shell.open_zone(zone_id, "files-pw").unwrap() else {
        panic!("expected Files zone");
    };
    let files = vault.list_files(None).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, "note.txt");
}

#[test]
fn seeds_zone_round_trips_through_shell() {
    let dir = tempfile::tempdir().unwrap();
    let shell = Shell::create(
        dir.path().join("shell.vault"),
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();

    let zone_id = shell
        .create_zone(ZoneKind::Seeds, "My Seeds", "🌱", "seeds-pw", test_params())
        .unwrap();

    let OpenZone::Seeds(vault) = shell.open_zone(zone_id, "seeds-pw").unwrap() else {
        panic!("expected Seeds zone");
    };
    vault
        .add_seed(&NewSeedEntry {
            label: "Ledger Nano".into(),
            network: "Bitcoin".into(),
            seed_phrase: "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".into(),
            derivation_path: "m/44'/0'/0'/0/0".into(),
            notes: String::new(),
        })
        .unwrap();
    shell.save_zone(zone_id, &OpenZone::Seeds(vault)).unwrap();

    let OpenZone::Seeds(vault) = shell.open_zone(zone_id, "seeds-pw").unwrap() else {
        panic!("expected Seeds zone");
    };
    let seeds = vault.list_seeds().unwrap();
    assert_eq!(seeds.len(), 1);
    assert_eq!(seeds[0].label, "Ledger Nano");
}

#[test]
fn ledger_zone_round_trips_through_shell() {
    let dir = tempfile::tempdir().unwrap();
    let shell = Shell::create(
        dir.path().join("shell.vault"),
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();

    let zone_id = shell
        .create_zone(
            ZoneKind::Ledger,
            "My Balance",
            "💰",
            "ledger-pw",
            test_params(),
        )
        .unwrap();

    let OpenZone::Ledger(vault) = shell.open_zone(zone_id, "ledger-pw").unwrap() else {
        panic!("expected Ledger zone");
    };
    vault.add_transaction(70_096, "Salary", None).unwrap();
    vault.add_transaction(-50_022, "Rent", None).unwrap();
    shell.save_zone(zone_id, &OpenZone::Ledger(vault)).unwrap();

    let OpenZone::Ledger(vault) = shell.open_zone(zone_id, "ledger-pw").unwrap() else {
        panic!("expected Ledger zone");
    };
    let txs = vault.list_transactions().unwrap();
    assert_eq!(txs.len(), 2);
    assert_eq!(vault.total_cents().unwrap(), 70_096 - 50_022);
}

#[test]
fn zones_have_completely_independent_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let shell = Shell::create(
        dir.path().join("shell.vault"),
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();

    let accounts_id = shell
        .create_zone(
            ZoneKind::Accounts,
            "Accounts",
            "🔑",
            "accounts-pw",
            test_params(),
        )
        .unwrap();
    let files_id = shell
        .create_zone(ZoneKind::Files, "Files", "🗂", "files-pw", test_params())
        .unwrap();

    // Each zone's own password fails on the other zone.
    match shell.open_zone(accounts_id, "files-pw") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword"),
    }
    match shell.open_zone(files_id, "accounts-pw") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword"),
    }

    // The shell password itself does not open either zone.
    match shell.open_zone(accounts_id, "shell-pw") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword"),
    }
}

#[test]
fn wrong_shell_password_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    {
        let shell = Shell::create(
            &shell_path,
            "correct-shell-pw",
            Some("correct-shell-pw-recovery"),
            test_params(),
        )
        .unwrap();
        shell
            .create_zone(
                ZoneKind::Accounts,
                "Accounts",
                "🔑",
                "zone-pw",
                test_params(),
            )
            .unwrap();
    }

    match Shell::open(&shell_path, "wrong-shell-pw") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword"),
    }
}

#[test]
fn shell_opens_with_primary_password_only() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    Shell::create(
        &shell_path,
        "primary-pw",
        Some("recovery-pw"),
        test_params(),
    )
    .unwrap();

    Shell::open(&shell_path, "primary-pw").unwrap();

    match Shell::open(&shell_path, "neither-of-them") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword"),
    }
}

/// The recovery password is a real, valid password for its own slot — but
/// it must not be usable to unlock the Shell directly, only to reset the
/// primary password via `Shell::change_password`. This is what makes it
/// meaningfully different from just having two equally-privileged
/// passwords: the recovery secret stays unexercised until an actual
/// emergency, and using it always leaves an unambiguous trace (a changed
/// primary password) rather than silent, repeatable direct access.
#[test]
fn recovery_password_cannot_open_shell_directly() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    Shell::create(
        &shell_path,
        "primary-pw",
        Some("recovery-pw"),
        test_params(),
    )
    .unwrap();

    match Shell::open(&shell_path, "recovery-pw") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("recovery password must not open the Shell directly"),
    }

    // It's still fully valid for resetting the primary password.
    Shell::change_password(
        &shell_path,
        "recovery-pw",
        PRIMARY_SLOT,
        "new-primary",
        test_params(),
    )
    .unwrap();
    Shell::open(&shell_path, "new-primary").unwrap();
}

#[test]
fn forgotten_primary_password_is_recoverable_via_recovery_password() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(
        &shell_path,
        "old-primary",
        Some("recovery-pw"),
        test_params(),
    )
    .unwrap();

    // "Forgot" the primary password: reset it using the recovery password.
    Shell::change_password(
        &shell_path,
        "recovery-pw",
        PRIMARY_SLOT,
        "new-primary",
        test_params(),
    )
    .unwrap();
    drop(shell);

    // New primary works; old primary no longer does; the recovery password
    // is still valid for further resets but still can't open directly.
    Shell::open(&shell_path, "new-primary").unwrap();
    match Shell::open(&shell_path, "recovery-pw") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("recovery password must not open the Shell directly"),
    }
    match Shell::open(&shell_path, "old-primary") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected old primary password to be rejected after reset"),
    }
}

#[test]
fn forgotten_recovery_password_is_resettable_via_primary_password() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(
        &shell_path,
        "primary-pw",
        Some("old-recovery"),
        test_params(),
    )
    .unwrap();

    // Works in the other direction too: reset the recovery slot using the
    // primary password.
    Shell::change_password(
        &shell_path,
        "primary-pw",
        RECOVERY_SLOT,
        "new-recovery",
        test_params(),
    )
    .unwrap();
    drop(shell);

    Shell::open(&shell_path, "primary-pw").unwrap();

    // Neither recovery password ever opens the Shell directly — check
    // "new-recovery" and "old-recovery" the only way that's meaningful for
    // a recovery-only slot: whether it's accepted as the known password for
    // a further reset.
    Shell::change_password(
        &shell_path,
        "new-recovery",
        PRIMARY_SLOT,
        "primary-pw",
        test_params(),
    )
    .unwrap();
    match Shell::change_password(
        &shell_path,
        "old-recovery",
        PRIMARY_SLOT,
        "primary-pw",
        test_params(),
    ) {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected old recovery password to be rejected after reset"),
    }
}

#[test]
fn changing_a_password_does_not_disturb_zone_data() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(
        &shell_path,
        "primary-pw",
        Some("recovery-pw"),
        test_params(),
    )
    .unwrap();

    let zone_id = shell
        .create_zone(
            ZoneKind::Accounts,
            "Accounts",
            "🔑",
            "zone-pw",
            test_params(),
        )
        .unwrap();
    let OpenZone::Accounts(vault) = shell.open_zone(zone_id, "zone-pw").unwrap() else {
        panic!()
    };
    vault
        .add_entry(&NewEntry {
            title: "Survives".into(),
            ..Default::default()
        })
        .unwrap();
    shell
        .save_zone(zone_id, &OpenZone::Accounts(vault))
        .unwrap();

    Shell::change_password(
        &shell_path,
        "recovery-pw",
        PRIMARY_SLOT,
        "new-primary",
        test_params(),
    )
    .unwrap();
    drop(shell);

    let shell = Shell::open(&shell_path, "new-primary").unwrap();
    let OpenZone::Accounts(vault) = shell.open_zone(zone_id, "zone-pw").unwrap() else {
        panic!()
    };
    let entries = vault.list_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "Survives");
}

#[test]
fn change_password_requires_a_currently_valid_password() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(
        &shell_path,
        "primary-pw",
        Some("recovery-pw"),
        test_params(),
    )
    .unwrap();

    match Shell::change_password(&shell_path, "totally-wrong", PRIMARY_SLOT, "new-primary", test_params()) {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword — must not be able to reset without knowing any valid password"),
    }

    // Confirm nothing changed.
    drop(shell);
    Shell::open(&shell_path, "primary-pw").unwrap();
}

#[test]
fn strict_shell_has_no_recovery_slot_and_only_one_password_works() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    Shell::create(&shell_path, "only-password", None, test_params()).unwrap();

    Shell::open(&shell_path, "only-password").unwrap();
    match Shell::open(&shell_path, "anything-else") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword"),
    }

    // No recovery slot exists, so there is nothing a wrong "known" password
    // (or even the real one, targeting a slot that was never created) can
    // reset it via — change_password still requires a currently-valid
    // password, which "only-password" is, and it's free to *create* the
    // recovery slot for the first time here (see next test).
}

#[test]
fn recovery_slot_can_be_added_later_to_a_strict_shell() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(&shell_path, "only-password", None, test_params()).unwrap();

    // Changed their mind after the fact: add a recovery password using the
    // one password that already exists. change_password's underlying
    // set_slot creates the slot since it doesn't exist yet.
    Shell::change_password(
        &shell_path,
        "only-password",
        RECOVERY_SLOT,
        "added-later-recovery",
        test_params(),
    )
    .unwrap();
    drop(shell);

    Shell::open(&shell_path, "only-password").unwrap();
    match Shell::open(&shell_path, "added-later-recovery") {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("recovery password must not open the Shell directly, even freshly added"),
    }
    // But it is a real, valid recovery password: usable to reset primary.
    Shell::change_password(
        &shell_path,
        "added-later-recovery",
        PRIMARY_SLOT,
        "reset-primary",
        test_params(),
    )
    .unwrap();
    Shell::open(&shell_path, "reset-primary").unwrap();
}

#[test]
fn list_zones_reflects_multiple_zones_and_rename_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let shell = Shell::create(
        dir.path().join("shell.vault"),
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();

    let a = shell
        .create_zone(ZoneKind::Accounts, "Accounts", "🔑", "pw1", test_params())
        .unwrap();
    let f = shell
        .create_zone(ZoneKind::Files, "Files", "🗂", "pw2", test_params())
        .unwrap();

    let zones = shell.list_zones().unwrap();
    assert_eq!(zones.len(), 2);
    assert!(zones
        .iter()
        .any(|z| z.id == a && z.kind == "accounts" && z.label == "Accounts"));
    assert!(zones
        .iter()
        .any(|z| z.id == f && z.kind == "files" && z.label == "Files"));

    shell.rename_zone(a, "Renamed").unwrap();
    let zones = shell.list_zones().unwrap();
    assert!(zones.iter().any(|z| z.id == a && z.label == "Renamed"));

    shell.delete_zone(f).unwrap();
    let zones = shell.list_zones().unwrap();
    assert_eq!(zones.len(), 1);
    assert_eq!(zones[0].id, a);
}

#[test]
fn export_zone_standalone_produces_independently_openable_file_with_same_password() {
    let dir = tempfile::tempdir().unwrap();
    let shell = Shell::create(
        dir.path().join("shell.vault"),
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();

    let zone_id = shell
        .create_zone(
            ZoneKind::Accounts,
            "Accounts",
            "🔑",
            "zone-pw",
            test_params(),
        )
        .unwrap();
    let OpenZone::Accounts(vault) = shell.open_zone(zone_id, "zone-pw").unwrap() else {
        panic!("expected Accounts zone");
    };
    vault
        .add_entry(&NewEntry {
            title: "Exported Entry".into(),
            ..Default::default()
        })
        .unwrap();
    shell
        .save_zone(zone_id, &OpenZone::Accounts(vault))
        .unwrap();

    let dest_path = dir.path().join("standalone").join("accounts-export.vault");
    let kind_label = shell
        .export_zone_standalone(zone_id, "zone-pw", &dest_path, test_params())
        .unwrap();
    assert_eq!(kind_label, "accounts");
    assert!(dest_path.exists());

    // Openable independently of the Shell, with the same zone password.
    let standalone = vault_core::Vault::open(&dest_path, "zone-pw").unwrap();
    let entries = standalone.list_entries().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "Exported Entry");

    // A wrong password is still rejected on the standalone copy.
    let err = vault_core::Vault::open(&dest_path, "wrong-pw").unwrap_err();
    assert!(matches!(err, VaultError::InvalidPassword));
}

#[test]
fn export_zone_standalone_requires_the_correct_zone_password() {
    let dir = tempfile::tempdir().unwrap();
    let shell = Shell::create(
        dir.path().join("shell.vault"),
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();
    let zone_id = shell
        .create_zone(
            ZoneKind::Accounts,
            "Accounts",
            "🔑",
            "zone-pw",
            test_params(),
        )
        .unwrap();

    let dest_path = dir.path().join("export.vault");
    match shell.export_zone_standalone(zone_id, "wrong-pw", &dest_path, test_params()) {
        Err(VaultError::InvalidPassword) => {}
        _ => panic!("expected InvalidPassword"),
    }
    assert!(!dest_path.exists());
}

/// The whole point of this architecture: after creating several zones,
/// nothing except the Shell's own file (+ its metadata sidecar) may exist
/// anywhere on disk. No `accounts.vault`, no `files.vault`, nothing that
/// would leak a zone's existence to someone browsing the filesystem
/// directly.
#[test]
fn no_files_other_than_the_shell_itself_ever_appear_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let shell_path = dir.path().join("shell.vault");
    let shell = Shell::create(
        &shell_path,
        "shell-pw",
        Some("shell-pw-recovery"),
        test_params(),
    )
    .unwrap();

    let accounts_id = shell
        .create_zone(
            ZoneKind::Accounts,
            "Secret Accounts",
            "🔑",
            "pw1",
            test_params(),
        )
        .unwrap();
    let files_id = shell
        .create_zone(ZoneKind::Files, "Secret Files", "🗂", "pw2", test_params())
        .unwrap();

    let OpenZone::Accounts(vault) = shell.open_zone(accounts_id, "pw1").unwrap() else {
        panic!()
    };
    vault
        .add_entry(&NewEntry {
            title: "x".into(),
            ..Default::default()
        })
        .unwrap();
    shell
        .save_zone(accounts_id, &OpenZone::Accounts(vault))
        .unwrap();

    let OpenZone::Files(vault) = shell.open_zone(files_id, "pw2").unwrap() else {
        panic!()
    };
    vault.add_file(None, "secret.txt", b"data").unwrap();
    shell.save_zone(files_id, &OpenZone::Files(vault)).unwrap();

    let entries: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();

    assert_eq!(
        entries.len(),
        2,
        "expected exactly the shell file + its meta sidecar, found: {entries:?}"
    );
    assert!(entries.iter().any(|n| n == "shell.vault"));
    assert!(entries.iter().any(|n| n == "shell.vault.meta.json"));
    assert!(!entries.iter().any(|n| n.contains("accounts")));
    assert!(!entries.iter().any(|n| n.contains("files")));
}
