#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// CIPHERDEN desktop shell. All cryptography and storage logic lives in
// `vault-core`; this crate only wires that core to Tauri IPC commands and
// enforces the app-level policies the spec requires (auto-lock, clipboard
// auto-clear, no plaintext left behind).
//
// Architecture: a single `Shell` (the one visible file on disk) holds a
// list of "zones" (Accounts, Files, ...), each stored as an opaque
// encrypted blob with its own independent password — see
// `vault_core::shell` for the full rationale. This file manages two pieces
// of state: the Shell itself, and a map of currently-unlocked zone
// sessions. Every mutating zone command re-persists that zone into the
// Shell immediately afterward (`with_accounts_zone`/`with_files_zone` with
// `mutate = true`), so there is no separate "save" step and no window
// where a crash could lose data that isn't also lost by SQLite itself.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::{engine::general_purpose::STANDARD, Engine};
use rand::Rng;
use serde::Serialize;
use sysinfo::{Disks, ProcessesToUpdate, System as Sysinfo};
use tauri::State;
use vault_core::files::{FileMeta, Folder};
use vault_core::shell::{OpenZone, ZoneKind, ZoneMeta};
use vault_core::store::{Entry, NewEntry};
use vault_core::{
    import, Argon2Params, FileVault, LedgerVault, NewSeedEntry, SeedEntry, SeedVault, Shell,
    Transaction, Vault, VaultError,
};

const AUTO_LOCK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CLIPBOARD_CLEAR_DELAY: Duration = Duration::from_secs(20);
const LOCK_CHECK_INTERVAL: Duration = Duration::from_secs(1);

fn to_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// --- Shell state -----------------------------------------------------------

struct ShellSlot {
    open: Option<Shell>,
    // Tracked purely so the read-only System zone can report the Shell
    // file's size and free disk space — never used for anything
    // authentication-related.
    open_path: Option<PathBuf>,
    last_activity: Instant,
}

impl ShellSlot {
    fn new() -> Self {
        Self {
            open: None,
            open_path: None,
            last_activity: Instant::now(),
        }
    }
}

type ShellState = Arc<Mutex<ShellSlot>>;

// --- Failed-unlock-attempt tracking (for the System zone's HUD only) -------
//
// In-memory only — resets on process restart, never written to disk. Counts
// strictly VaultError::InvalidPassword responses from open_shell, never a
// bad path/IO error, so it reflects real wrong-password attempts and
// nothing else.
struct AttemptLog {
    total_failed: u32,
    recent: VecDeque<Instant>,
}

const ATTEMPT_RECENT_WINDOW: Duration = Duration::from_secs(5 * 60);

impl AttemptLog {
    fn new() -> Self {
        Self {
            total_failed: 0,
            recent: VecDeque::new(),
        }
    }
    fn record_failure(&mut self) {
        self.total_failed += 1;
        self.recent.push_back(Instant::now());
        self.prune();
    }
    fn prune(&mut self) {
        while let Some(&front) = self.recent.front() {
            if front.elapsed() > ATTEMPT_RECENT_WINDOW {
                self.recent.pop_front();
            } else {
                break;
            }
        }
    }
    fn recent_count(&mut self) -> u32 {
        self.prune();
        self.recent.len() as u32
    }
}

type AttemptsState = Arc<Mutex<AttemptLog>>;
type SysState = Arc<Mutex<Sysinfo>>;

struct ZoneSession {
    zone: OpenZone,
    last_activity: Instant,
}

type SessionsState = Arc<Mutex<HashMap<i64, ZoneSession>>>;

/// Run `f` against the Accounts vault for `zone_id`. If `mutate` is true,
/// re-persists the zone into the Shell afterward.
fn with_accounts_zone<T>(
    sessions: &SessionsState,
    shell: &ShellState,
    zone_id: i64,
    mutate: bool,
    f: impl FnOnce(&Vault) -> Result<T, String>,
) -> Result<T, String> {
    let mut sessions = sessions.lock().map_err(to_err)?;
    let session = sessions.get_mut(&zone_id).ok_or("zone is locked")?;
    session.last_activity = Instant::now();
    let OpenZone::Accounts(vault) = &session.zone else {
        return Err("this zone is not an Accounts zone".into());
    };
    let result = f(vault)?;
    if mutate {
        persist_zone(shell, zone_id, &session.zone)?;
    }
    Ok(result)
}

fn with_files_zone<T>(
    sessions: &SessionsState,
    shell: &ShellState,
    zone_id: i64,
    mutate: bool,
    f: impl FnOnce(&FileVault) -> Result<T, String>,
) -> Result<T, String> {
    let mut sessions = sessions.lock().map_err(to_err)?;
    let session = sessions.get_mut(&zone_id).ok_or("zone is locked")?;
    session.last_activity = Instant::now();
    let OpenZone::Files(vault) = &session.zone else {
        return Err("this zone is not a Files zone".into());
    };
    let result = f(vault)?;
    if mutate {
        persist_zone(shell, zone_id, &session.zone)?;
    }
    Ok(result)
}

fn with_seeds_zone<T>(
    sessions: &SessionsState,
    shell: &ShellState,
    zone_id: i64,
    mutate: bool,
    f: impl FnOnce(&SeedVault) -> Result<T, String>,
) -> Result<T, String> {
    let mut sessions = sessions.lock().map_err(to_err)?;
    let session = sessions.get_mut(&zone_id).ok_or("zone is locked")?;
    session.last_activity = Instant::now();
    let OpenZone::Seeds(vault) = &session.zone else {
        return Err("this zone is not a Seeds zone".into());
    };
    let result = f(vault)?;
    if mutate {
        persist_zone(shell, zone_id, &session.zone)?;
    }
    Ok(result)
}

fn with_ledger_zone<T>(
    sessions: &SessionsState,
    shell: &ShellState,
    zone_id: i64,
    mutate: bool,
    f: impl FnOnce(&LedgerVault) -> Result<T, String>,
) -> Result<T, String> {
    let mut sessions = sessions.lock().map_err(to_err)?;
    let session = sessions.get_mut(&zone_id).ok_or("zone is locked")?;
    session.last_activity = Instant::now();
    let OpenZone::Ledger(vault) = &session.zone else {
        return Err("this zone is not a Ledger zone".into());
    };
    let result = f(vault)?;
    if mutate {
        persist_zone(shell, zone_id, &session.zone)?;
    }
    Ok(result)
}

fn persist_zone(shell: &ShellState, zone_id: i64, zone: &OpenZone) -> Result<(), String> {
    let mut shell_guard = shell.lock().map_err(to_err)?;
    shell_guard.last_activity = Instant::now();
    let shell = shell_guard.open.as_ref().ok_or("shell is locked")?;
    shell.save_zone(zone_id, zone).map_err(to_err)
}

// --- Shell lifecycle ---------------------------------------------------

#[tauri::command]
fn create_shell(
    state: State<'_, ShellState>,
    path: String,
    primary_password: String,
    recovery_password: Option<String>,
) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);
    let shell = Shell::create(
        path_buf.clone(),
        &primary_password,
        recovery_password.as_deref(),
        Argon2Params::standard(),
    )
    .map_err(to_err)?;
    let mut guard = state.lock().map_err(to_err)?;
    guard.open = Some(shell);
    guard.open_path = Some(path_buf);
    guard.last_activity = Instant::now();
    Ok(())
}

#[tauri::command]
fn open_shell(
    state: State<'_, ShellState>,
    attempts: State<'_, AttemptsState>,
    path: String,
    password: String,
) -> Result<(), String> {
    let path_buf = PathBuf::from(&path);
    match Shell::open(path_buf.clone(), &password) {
        Ok(shell) => {
            let mut guard = state.lock().map_err(to_err)?;
            guard.open = Some(shell);
            guard.open_path = Some(path_buf);
            guard.last_activity = Instant::now();
            Ok(())
        }
        Err(e) => {
            if matches!(e, VaultError::InvalidPassword) {
                if let Ok(mut log) = attempts.lock() {
                    log.record_failure();
                }
            }
            Err(to_err(e))
        }
    }
}

/// Set a new value for the primary or recovery password slot, authenticated
/// by *any* currently-valid password (that slot's own, or the other one) —
/// works whether the Shell is currently unlocked or not, since it only
/// needs a valid password, not an open session.
#[tauri::command]
fn change_shell_password(
    path: String,
    known_password: String,
    slot: String,
    new_password: String,
) -> Result<(), String> {
    let slot_name = match slot.as_str() {
        "primary" => vault_core::PRIMARY_SLOT,
        "recovery" => vault_core::RECOVERY_SLOT,
        other => return Err(format!("unknown password slot: {other}")),
    };
    // Never goes through Shell::open (which only accepts the primary
    // password) — change_password only touches the key-slot sidecar file
    // directly, so recovering via the *recovery* password still works.
    Shell::change_password(
        PathBuf::from(path),
        &known_password,
        slot_name,
        &new_password,
        Argon2Params::standard(),
    )
    .map_err(to_err)
}

#[tauri::command]
fn lock_shell(
    shell: State<'_, ShellState>,
    sessions: State<'_, SessionsState>,
) -> Result<(), String> {
    // Locking the Shell locks every open zone too: without the Shell there
    // is nowhere to persist further changes, and there is no reason to
    // keep decrypted zone data sitting in memory once its container locks.
    let mut sessions = sessions.lock().map_err(to_err)?;
    sessions.clear();
    let mut guard = shell.lock().map_err(to_err)?;
    guard.open = None;
    guard.open_path = None;
    Ok(())
}

#[tauri::command]
fn is_shell_unlocked(state: State<'_, ShellState>) -> Result<bool, String> {
    let guard = state.lock().map_err(to_err)?;
    Ok(guard.open.is_some())
}

/// Emergency quick-exit: same as `lock_shell` (clear every open zone
/// session and drop the Shell key from memory) and then terminate the
/// whole process immediately, for the moment the drive needs to be pulled
/// right now with no time for a normal shutdown. There is nothing to flush
/// first — every mutating zone command already persists synchronously via
/// SQLite's own atomic commit, so no data is buffered in memory waiting to
/// be saved.
#[tauri::command]
fn emergency_exit(shell: State<'_, ShellState>, sessions: State<'_, SessionsState>) {
    if let Ok(mut sessions) = sessions.lock() {
        sessions.clear();
    }
    if let Ok(mut guard) = shell.lock() {
        guard.open = None;
        guard.open_path = None;
    }
    std::process::exit(0);
}

// --- System zone: a read-only security/status dashboard --------------------
//
// Everything here is either a real, directly-measured number (file size,
// free disk space, this process's own RAM/CPU, a same-process failed-
// attempt counter) or a static fact about the app's own crypto parameters.
// Nothing here is fabricated, and nothing here is a control — nothing this
// command's data feeds ever exposes a mutating action.

#[derive(Serialize)]
struct SystemStatus {
    shell_open: bool,
    shell_file_bytes: Option<u64>,
    disk_free_bytes: Option<u64>,
    disk_total_bytes: Option<u64>,
    process_ram_bytes: u64,
    process_cpu_percent: f32,
    failed_attempts_total: u32,
    failed_attempts_recent: u32,
    attempt_window_seconds: u64,
    argon2_m_cost_kib: u32,
    argon2_t_cost: u32,
    argon2_p_cost: u32,
    auto_lock_seconds: u64,
}

/// Finds the disk whose mount point is the longest matching prefix of `dir`
/// (so `/Volumes/MySSD/x` resolves to the `/Volumes/MySSD` volume, not `/`).
fn disk_space_for(dir: &Path) -> (Option<u64>, Option<u64>) {
    let disks = Disks::new_with_refreshed_list();
    let mut best: Option<(&Path, u64, u64)> = None;
    for disk in disks.list() {
        let mp = disk.mount_point();
        if dir.starts_with(mp) {
            let is_better = best.map(|(b, _, _)| mp.as_os_str().len() > b.as_os_str().len());
            if is_better.unwrap_or(true) {
                best = Some((mp, disk.available_space(), disk.total_space()));
            }
        }
    }
    match best {
        Some((_, free, total)) => (Some(free), Some(total)),
        None => (None, None),
    }
}

#[tauri::command]
fn get_system_status(
    shell: State<'_, ShellState>,
    attempts: State<'_, AttemptsState>,
    sys: State<'_, SysState>,
) -> Result<SystemStatus, String> {
    let (shell_open, shell_path) = {
        let guard = shell.lock().map_err(to_err)?;
        (guard.open.is_some(), guard.open_path.clone())
    };

    let shell_file_bytes = shell_path
        .as_ref()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|m| m.len());

    let (disk_free_bytes, disk_total_bytes) = shell_path
        .as_ref()
        .and_then(|p| p.parent())
        .map(disk_space_for)
        .unwrap_or((None, None));

    let (process_ram_bytes, process_cpu_percent) = {
        let pid = sysinfo::get_current_pid().map_err(|e| e.to_string())?;
        let mut sys = sys.lock().map_err(to_err)?;
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        sys.process(pid)
            .map(|p| (p.memory(), p.cpu_usage()))
            .unwrap_or((0, 0.0))
    };

    let (failed_attempts_total, failed_attempts_recent) = {
        let mut log = attempts.lock().map_err(to_err)?;
        (log.total_failed, log.recent_count())
    };

    let params = Argon2Params::standard();

    Ok(SystemStatus {
        shell_open,
        shell_file_bytes,
        disk_free_bytes,
        disk_total_bytes,
        process_ram_bytes,
        process_cpu_percent,
        failed_attempts_total,
        failed_attempts_recent,
        attempt_window_seconds: ATTEMPT_RECENT_WINDOW.as_secs(),
        argon2_m_cost_kib: params.m_cost_kib,
        argon2_t_cost: params.t_cost,
        argon2_p_cost: params.p_cost,
        auto_lock_seconds: AUTO_LOCK_TIMEOUT.as_secs(),
    })
}

#[tauri::command]
fn export_shell_backup(state: State<'_, ShellState>, dest_dir: String) -> Result<String, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.last_activity = Instant::now();
    let shell = guard.open.as_ref().ok_or("shell is locked")?;
    shell
        .export_backup(PathBuf::from(dest_dir))
        .map(|p| p.display().to_string())
        .map_err(to_err)
}

/// Opt-in safety valve: export a single zone to its own standalone
/// encrypted file, openable later with the same zone password even
/// without the Shell. Deliberately re-introduces the on-disk existence
/// exposure the Shell/zones architecture otherwise avoids — a one-time,
/// explicit user action, never automatic.
#[tauri::command]
fn export_zone_standalone(
    state: State<'_, ShellState>,
    zone_id: i64,
    zone_password: String,
    dest_path: String,
) -> Result<String, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.last_activity = Instant::now();
    let shell = guard.open.as_ref().ok_or("shell is locked")?;
    shell
        .export_zone_standalone(
            zone_id,
            &zone_password,
            PathBuf::from(dest_path),
            Argon2Params::standard(),
        )
        .map(|kind| kind.to_string())
        .map_err(to_err)
}

// --- Zone management -----------------------------------------------------

#[tauri::command]
fn list_zones(state: State<'_, ShellState>) -> Result<Vec<ZoneMeta>, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.last_activity = Instant::now();
    let shell = guard.open.as_ref().ok_or("shell is locked")?;
    shell.list_zones().map_err(to_err)
}

#[tauri::command]
fn create_zone(
    state: State<'_, ShellState>,
    kind: String,
    label: String,
    icon: String,
    zone_password: String,
) -> Result<i64, String> {
    let kind = ZoneKind::parse(&kind).map_err(to_err)?;
    let mut guard = state.lock().map_err(to_err)?;
    guard.last_activity = Instant::now();
    let shell = guard.open.as_ref().ok_or("shell is locked")?;
    shell
        .create_zone(
            kind,
            &label,
            &icon,
            &zone_password,
            Argon2Params::standard(),
        )
        .map_err(to_err)
}

#[tauri::command]
fn rename_zone(
    state: State<'_, ShellState>,
    zone_id: i64,
    new_label: String,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.last_activity = Instant::now();
    let shell = guard.open.as_ref().ok_or("shell is locked")?;
    shell.rename_zone(zone_id, &new_label).map_err(to_err)
}

#[tauri::command]
fn delete_zone(
    state: State<'_, ShellState>,
    sessions: State<'_, SessionsState>,
    zone_id: i64,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.last_activity = Instant::now();
    let shell = guard.open.as_ref().ok_or("shell is locked")?;
    shell.delete_zone(zone_id).map_err(to_err)?;
    sessions.lock().map_err(to_err)?.remove(&zone_id);
    Ok(())
}

#[tauri::command]
fn open_zone(
    shell: State<'_, ShellState>,
    sessions: State<'_, SessionsState>,
    zone_id: i64,
    zone_password: String,
) -> Result<String, String> {
    let mut shell_guard = shell.lock().map_err(to_err)?;
    shell_guard.last_activity = Instant::now();
    let shell_ref = shell_guard.open.as_ref().ok_or("shell is locked")?;
    let zone = shell_ref
        .open_zone(zone_id, &zone_password)
        .map_err(to_err)?;

    let kind_label = match &zone {
        OpenZone::Accounts(_) => "accounts",
        OpenZone::Files(_) => "files",
        OpenZone::Seeds(_) => "seeds",
        OpenZone::Ledger(_) => "ledger",
    };

    sessions.lock().map_err(to_err)?.insert(
        zone_id,
        ZoneSession {
            zone,
            last_activity: Instant::now(),
        },
    );

    Ok(kind_label.to_string())
}

#[tauri::command]
fn lock_zone(sessions: State<'_, SessionsState>, zone_id: i64) -> Result<(), String> {
    sessions.lock().map_err(to_err)?.remove(&zone_id);
    Ok(())
}

#[tauri::command]
fn is_zone_unlocked(sessions: State<'_, SessionsState>, zone_id: i64) -> Result<bool, String> {
    Ok(sessions.lock().map_err(to_err)?.contains_key(&zone_id))
}

// --- Accounts zone commands ----------------------------------------------

#[tauri::command]
fn list_entries(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
) -> Result<Vec<Entry>, String> {
    with_accounts_zone(&sessions, &shell, zone_id, false, |v| {
        v.list_entries().map_err(to_err)
    })
}

#[tauri::command]
fn search_entries(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    query: String,
) -> Result<Vec<Entry>, String> {
    with_accounts_zone(&sessions, &shell, zone_id, false, |v| {
        v.search(&query).map_err(to_err)
    })
}

#[tauri::command]
fn add_entry(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    entry: NewEntry,
) -> Result<i64, String> {
    with_accounts_zone(&sessions, &shell, zone_id, true, |v| {
        v.add_entry(&entry).map_err(to_err)
    })
}

#[tauri::command]
fn update_entry(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    entry: NewEntry,
) -> Result<(), String> {
    with_accounts_zone(&sessions, &shell, zone_id, true, |v| {
        v.update_entry(id, &entry).map_err(to_err)
    })
}

#[tauri::command]
fn delete_entry(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
) -> Result<(), String> {
    with_accounts_zone(&sessions, &shell, zone_id, true, |v| {
        v.delete_entry(id).map_err(to_err)
    })
}

#[tauri::command]
fn export_csv(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    ids: Vec<i64>,
    dest_path: String,
) -> Result<usize, String> {
    with_accounts_zone(&sessions, &shell, zone_id, false, |v| {
        vault_core::export::export_csv(v, &ids, PathBuf::from(dest_path)).map_err(to_err)
    })
}

#[tauri::command]
fn import_csv(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    path: String,
) -> Result<usize, String> {
    with_accounts_zone(&sessions, &shell, zone_id, true, |v| {
        import::import_csv(v, PathBuf::from(path)).map_err(to_err)
    })
}

#[tauri::command]
fn import_kdbx(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    path: String,
    kdbx_password: String,
) -> Result<usize, String> {
    with_accounts_zone(&sessions, &shell, zone_id, true, |v| {
        import::import_kdbx(v, PathBuf::from(path), &kdbx_password).map_err(to_err)
    })
}

#[derive(Serialize)]
struct GeneratedPassword {
    password: String,
}

#[tauri::command]
fn generate_password(length: usize, use_symbols: bool) -> GeneratedPassword {
    const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const DIGITS: &[u8] = b"0123456789";
    const SYMBOLS: &[u8] = b"!@#$%^&*()-_=+[]{};:,.?";

    let mut alphabet: Vec<u8> = Vec::new();
    alphabet.extend_from_slice(LOWER);
    alphabet.extend_from_slice(UPPER);
    alphabet.extend_from_slice(DIGITS);
    if use_symbols {
        alphabet.extend_from_slice(SYMBOLS);
    }

    let length = length.clamp(8, 128);
    let mut rng = rand::thread_rng();
    let password: String = (0..length)
        .map(|_| alphabet[rng.gen_range(0..alphabet.len())] as char)
        .collect();

    GeneratedPassword { password }
}

/// Copies `text` to the OS clipboard and schedules an automatic clear after
/// `CLIPBOARD_CLEAR_DELAY`, but only if the clipboard still holds exactly
/// what we put there (so we never clobber something the user copied since).
#[tauri::command]
fn copy_to_clipboard_with_autoclear(text: String) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(to_err)?;
    clipboard.set_text(text.clone()).map_err(to_err)?;

    std::thread::spawn(move || {
        std::thread::sleep(CLIPBOARD_CLEAR_DELAY);
        if let Ok(mut clipboard) = arboard::Clipboard::new() {
            if clipboard.get_text().map(|t| t == text).unwrap_or(false) {
                let _ = clipboard.set_text(String::new());
            }
        }
    });

    Ok(())
}

// --- Files zone commands --------------------------------------------------

#[tauri::command]
fn list_files(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    folder_id: Option<i64>,
) -> Result<Vec<FileMeta>, String> {
    with_files_zone(&sessions, &shell, zone_id, false, |v| {
        v.list_files(folder_id).map_err(to_err)
    })
}

#[tauri::command]
fn list_folders(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    parent_id: Option<i64>,
) -> Result<Vec<Folder>, String> {
    with_files_zone(&sessions, &shell, zone_id, false, |v| {
        v.list_folders(parent_id).map_err(to_err)
    })
}

#[tauri::command]
fn create_folder(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    parent_id: Option<i64>,
    name: String,
) -> Result<i64, String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        v.create_folder(parent_id, &name).map_err(to_err)
    })
}

#[tauri::command]
fn rename_folder(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    new_name: String,
) -> Result<(), String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        v.rename_folder(id, &new_name).map_err(to_err)
    })
}

#[tauri::command]
fn delete_folder(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
) -> Result<(), String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        v.delete_folder(id).map_err(to_err)
    })
}

#[tauri::command]
fn move_folder(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    new_parent_id: Option<i64>,
) -> Result<(), String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        v.move_folder(id, new_parent_id).map_err(to_err)
    })
}

#[tauri::command]
fn set_folder_pinned(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    pinned: bool,
) -> Result<(), String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        v.set_folder_pinned(id, pinned).map_err(to_err)
    })
}

#[tauri::command]
fn move_file(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    folder_id: Option<i64>,
) -> Result<(), String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        v.move_file(id, folder_id).map_err(to_err)
    })
}

#[tauri::command]
fn set_file_pinned(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    pinned: bool,
) -> Result<(), String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        v.set_file_pinned(id, pinned).map_err(to_err)
    })
}

#[tauri::command]
fn add_files(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    folder_id: Option<i64>,
    paths: Vec<String>,
) -> Result<usize, String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        let mut added = 0;
        for path in &paths {
            let path = PathBuf::from(path);
            let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            v.add_file(folder_id, &name, &data).map_err(to_err)?;
            added += 1;
        }
        Ok(added)
    })
}

#[tauri::command]
fn extract_file(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    dest_path: String,
) -> Result<(), String> {
    with_files_zone(&sessions, &shell, zone_id, false, |v| {
        let (_meta, data) = v.read_file(id).map_err(to_err)?.ok_or("file not found")?;
        std::fs::write(PathBuf::from(dest_path), data).map_err(to_err)
    })
}

#[derive(Serialize)]
struct FilePreview {
    name: String,
    mime: String,
    size: i64,
    data_base64: String,
}

/// Returns a file's contents in-memory (base64-encoded) for the frontend to
/// render directly — no decrypted copy ever touches the host filesystem,
/// unlike `extract_file`.
#[tauri::command]
fn read_file_preview(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
) -> Result<FilePreview, String> {
    with_files_zone(&sessions, &shell, zone_id, false, |v| {
        let (meta, data) = v.read_file(id).map_err(to_err)?.ok_or("file not found")?;
        Ok(FilePreview {
            mime: guess_mime(&meta.name).to_string(),
            name: meta.name,
            size: meta.size,
            data_base64: STANDARD.encode(&data),
        })
    })
}

fn guess_mime(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "csv" => "text/csv",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

#[tauri::command]
fn delete_file(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
) -> Result<(), String> {
    with_files_zone(&sessions, &shell, zone_id, true, |v| {
        v.delete_file(id).map_err(to_err)
    })
}

// --- Seeds zone commands ---------------------------------------------------

#[tauri::command]
fn list_seeds(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
) -> Result<Vec<SeedEntry>, String> {
    with_seeds_zone(&sessions, &shell, zone_id, false, |v| {
        v.list_seeds().map_err(to_err)
    })
}

#[tauri::command]
fn add_seed(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    entry: NewSeedEntry,
) -> Result<i64, String> {
    with_seeds_zone(&sessions, &shell, zone_id, true, |v| {
        v.add_seed(&entry).map_err(to_err)
    })
}

#[tauri::command]
fn update_seed(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    entry: NewSeedEntry,
) -> Result<(), String> {
    with_seeds_zone(&sessions, &shell, zone_id, true, |v| {
        v.update_seed(id, &entry).map_err(to_err)
    })
}

#[tauri::command]
fn delete_seed(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
) -> Result<(), String> {
    with_seeds_zone(&sessions, &shell, zone_id, true, |v| {
        v.delete_seed(id).map_err(to_err)
    })
}

// --- Ledger (Balance) zone commands -----------------------------------------

#[derive(Serialize)]
struct LedgerSnapshot {
    transactions: Vec<Transaction>,
    total_cents: i64,
}

#[tauri::command]
fn list_transactions_with_total(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
) -> Result<LedgerSnapshot, String> {
    with_ledger_zone(&sessions, &shell, zone_id, false, |v| {
        Ok(LedgerSnapshot {
            transactions: v.list_transactions().map_err(to_err)?,
            total_cents: v.total_cents().map_err(to_err)?,
        })
    })
}

#[tauri::command]
fn add_transaction(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    amount_cents: i64,
    comment: String,
) -> Result<i64, String> {
    with_ledger_zone(&sessions, &shell, zone_id, true, |v| {
        v.add_transaction(amount_cents, &comment).map_err(to_err)
    })
}

#[tauri::command]
fn update_transaction(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
    amount_cents: i64,
    comment: String,
) -> Result<(), String> {
    with_ledger_zone(&sessions, &shell, zone_id, true, |v| {
        v.update_transaction(id, amount_cents, &comment)
            .map_err(to_err)
    })
}

#[tauri::command]
fn delete_transaction(
    sessions: State<'_, SessionsState>,
    shell: State<'_, ShellState>,
    zone_id: i64,
    id: i64,
) -> Result<(), String> {
    with_ledger_zone(&sessions, &shell, zone_id, true, |v| {
        v.delete_transaction(id).map_err(to_err)
    })
}

fn main() {
    let shell_state: ShellState = Arc::new(Mutex::new(ShellSlot::new()));
    let sessions_state: SessionsState = Arc::new(Mutex::new(HashMap::new()));
    let attempts_state: AttemptsState = Arc::new(Mutex::new(AttemptLog::new()));
    let sys_state: SysState = Arc::new(Mutex::new(Sysinfo::new()));

    // Auto-lock watcher: force-locks the Shell (and, with it, every open
    // zone) after inactivity, and independently evicts any single zone
    // session that's been idle even while the Shell itself stays unlocked.
    {
        let shell_watch = shell_state.clone();
        let sessions_watch = sessions_state.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(LOCK_CHECK_INTERVAL);

            let shell_timed_out = {
                if let Ok(guard) = shell_watch.lock() {
                    guard.open.is_some() && guard.last_activity.elapsed() > AUTO_LOCK_TIMEOUT
                } else {
                    false
                }
            };
            if shell_timed_out {
                if let Ok(mut sessions) = sessions_watch.lock() {
                    sessions.clear();
                }
                if let Ok(mut guard) = shell_watch.lock() {
                    guard.open = None;
                }
                continue;
            }

            if let Ok(mut sessions) = sessions_watch.lock() {
                sessions.retain(|_, s| s.last_activity.elapsed() <= AUTO_LOCK_TIMEOUT);
            }
        });
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(shell_state)
        .manage(sessions_state)
        .manage(attempts_state)
        .manage(sys_state)
        .invoke_handler(tauri::generate_handler![
            create_shell,
            open_shell,
            change_shell_password,
            lock_shell,
            is_shell_unlocked,
            emergency_exit,
            get_system_status,
            export_shell_backup,
            export_zone_standalone,
            list_zones,
            create_zone,
            rename_zone,
            delete_zone,
            open_zone,
            lock_zone,
            is_zone_unlocked,
            list_entries,
            search_entries,
            add_entry,
            update_entry,
            delete_entry,
            export_csv,
            import_csv,
            import_kdbx,
            generate_password,
            copy_to_clipboard_with_autoclear,
            list_files,
            list_folders,
            create_folder,
            rename_folder,
            delete_folder,
            move_folder,
            set_folder_pinned,
            move_file,
            set_file_pinned,
            add_files,
            extract_file,
            read_file_preview,
            delete_file,
            list_seeds,
            add_seed,
            update_seed,
            delete_seed,
            list_transactions_with_total,
            add_transaction,
            update_transaction,
            delete_transaction,
        ])
        .setup(|app| {
            // Loopback-only by construction: Tauri's webview talks to this
            // process over an internal IPC channel, not a TCP socket, so
            // there is no port to accidentally expose beyond localhost.
            let _ = app.handle();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running CIPHERDEN");
}
