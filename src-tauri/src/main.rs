// CIPHERDEN desktop shell. All cryptography and storage logic lives in
// `vault-core`; this crate only wires that core to Tauri IPC commands and
// enforces the app-level policies the spec requires (auto-lock, clipboard
// auto-clear, no plaintext left behind). Each "zone" (accounts, files, ...)
// is an independent encrypted container with its own lock state, mirroring
// vault-core's architecture.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;
use serde::Serialize;
use tauri::State;
use vault_core::files::FileMeta;
use vault_core::store::{Entry, NewEntry};
use vault_core::{import, Argon2Params, FileVault, Vault};

const AUTO_LOCK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CLIPBOARD_CLEAR_DELAY: Duration = Duration::from_secs(20);
const LOCK_CHECK_INTERVAL: Duration = Duration::from_secs(1);

struct ZoneState<T> {
    open: Option<T>,
    last_activity: Instant,
}

impl<T> ZoneState<T> {
    fn new() -> Self {
        Self {
            open: None,
            last_activity: Instant::now(),
        }
    }

    fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
}

type AccountsState = Arc<Mutex<ZoneState<Vault>>>;
type FilesState = Arc<Mutex<ZoneState<FileVault>>>;

fn to_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Spawn the background thread that force-locks a zone after
/// `AUTO_LOCK_TIMEOUT` of inactivity, regardless of what the frontend does.
fn spawn_lock_watcher<T: Send + 'static>(state: Arc<Mutex<ZoneState<T>>>) {
    std::thread::spawn(move || loop {
        std::thread::sleep(LOCK_CHECK_INTERVAL);
        if let Ok(mut guard) = state.lock() {
            if guard.open.is_some() && guard.last_activity.elapsed() > AUTO_LOCK_TIMEOUT {
                guard.open = None;
            }
        }
    });
}

// --- Accounts zone -----------------------------------------------------

#[tauri::command]
fn create_vault(
    state: State<'_, AccountsState>,
    path: String,
    password: String,
) -> Result<(), String> {
    let vault =
        Vault::create(PathBuf::from(path), &password, Argon2Params::standard()).map_err(to_err)?;
    let mut guard = state.lock().map_err(to_err)?;
    guard.open = Some(vault);
    guard.touch();
    Ok(())
}

#[tauri::command]
fn open_vault(
    state: State<'_, AccountsState>,
    path: String,
    password: String,
) -> Result<(), String> {
    let vault = Vault::open(PathBuf::from(path), &password).map_err(to_err)?;
    let mut guard = state.lock().map_err(to_err)?;
    guard.open = Some(vault);
    guard.touch();
    Ok(())
}

#[tauri::command]
fn lock_vault(state: State<'_, AccountsState>) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.open = None; // dropping the Vault drops the SQLCipher connection and zeroizes the key
    Ok(())
}

#[tauri::command]
fn is_unlocked(state: State<'_, AccountsState>) -> Result<bool, String> {
    let guard = state.lock().map_err(to_err)?;
    Ok(guard.open.is_some())
}

#[tauri::command]
fn list_entries(state: State<'_, AccountsState>) -> Result<Vec<Entry>, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    vault.list_entries().map_err(to_err)
}

#[tauri::command]
fn search_entries(state: State<'_, AccountsState>, query: String) -> Result<Vec<Entry>, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    vault.search(&query).map_err(to_err)
}

#[tauri::command]
fn add_entry(state: State<'_, AccountsState>, entry: NewEntry) -> Result<i64, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    vault.add_entry(&entry).map_err(to_err)
}

#[tauri::command]
fn update_entry(state: State<'_, AccountsState>, id: i64, entry: NewEntry) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    vault.update_entry(id, &entry).map_err(to_err)
}

#[tauri::command]
fn delete_entry(state: State<'_, AccountsState>, id: i64) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    vault.delete_entry(id).map_err(to_err)
}

#[tauri::command]
fn export_backup(state: State<'_, AccountsState>, dest_dir: String) -> Result<String, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    vault
        .export_backup(PathBuf::from(dest_dir))
        .map(|p| p.display().to_string())
        .map_err(to_err)
}

#[tauri::command]
fn export_csv(
    state: State<'_, AccountsState>,
    ids: Vec<i64>,
    dest_path: String,
) -> Result<usize, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    vault_core::export::export_csv(vault, &ids, PathBuf::from(dest_path)).map_err(to_err)
}

#[tauri::command]
fn import_csv(state: State<'_, AccountsState>, path: String) -> Result<usize, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    import::import_csv(vault, PathBuf::from(path)).map_err(to_err)
}

#[tauri::command]
fn import_kdbx(
    state: State<'_, AccountsState>,
    path: String,
    kdbx_password: String,
) -> Result<usize, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("vault is locked")?;
    import::import_kdbx(vault, PathBuf::from(path), &kdbx_password).map_err(to_err)
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

// --- Files zone ----------------------------------------------------------

#[tauri::command]
fn create_file_vault(
    state: State<'_, FilesState>,
    path: String,
    password: String,
) -> Result<(), String> {
    let vault = FileVault::create(PathBuf::from(path), &password, Argon2Params::standard())
        .map_err(to_err)?;
    let mut guard = state.lock().map_err(to_err)?;
    guard.open = Some(vault);
    guard.touch();
    Ok(())
}

#[tauri::command]
fn open_file_vault(
    state: State<'_, FilesState>,
    path: String,
    password: String,
) -> Result<(), String> {
    let vault = FileVault::open(PathBuf::from(path), &password).map_err(to_err)?;
    let mut guard = state.lock().map_err(to_err)?;
    guard.open = Some(vault);
    guard.touch();
    Ok(())
}

#[tauri::command]
fn lock_file_vault(state: State<'_, FilesState>) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.open = None;
    Ok(())
}

#[tauri::command]
fn is_file_vault_unlocked(state: State<'_, FilesState>) -> Result<bool, String> {
    let guard = state.lock().map_err(to_err)?;
    Ok(guard.open.is_some())
}

#[tauri::command]
fn list_files(state: State<'_, FilesState>) -> Result<Vec<FileMeta>, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("file vault is locked")?;
    vault.list_files().map_err(to_err)
}

/// Reads each source path from the host filesystem and stores its contents
/// as a new blob in the vault. Returns how many files were added; a source
/// path that fails to read is skipped rather than aborting the whole batch.
#[tauri::command]
fn add_files(state: State<'_, FilesState>, paths: Vec<String>) -> Result<usize, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("file vault is locked")?;

    let mut added = 0;
    for path in paths {
        let path = PathBuf::from(path);
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        vault.add_file(&name, &data).map_err(to_err)?;
        added += 1;
    }
    Ok(added)
}

/// Writes a stored file's contents out to `dest_path` on the host
/// filesystem (e.g. after the user picks a save location).
#[tauri::command]
fn extract_file(state: State<'_, FilesState>, id: i64, dest_path: String) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("file vault is locked")?;
    let (_meta, data) = vault
        .read_file(id)
        .map_err(to_err)?
        .ok_or("file not found")?;
    std::fs::write(PathBuf::from(dest_path), data).map_err(to_err)
}

#[tauri::command]
fn delete_file(state: State<'_, FilesState>, id: i64) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("file vault is locked")?;
    vault.delete_file(id).map_err(to_err)
}

#[tauri::command]
fn export_file_vault_backup(
    state: State<'_, FilesState>,
    dest_dir: String,
) -> Result<String, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.open.as_ref().ok_or("file vault is locked")?;
    vault
        .export_backup(PathBuf::from(dest_dir))
        .map(|p| p.display().to_string())
        .map_err(to_err)
}

fn main() {
    let accounts_state: AccountsState = Arc::new(Mutex::new(ZoneState::new()));
    let files_state: FilesState = Arc::new(Mutex::new(ZoneState::new()));
    spawn_lock_watcher(accounts_state.clone());
    spawn_lock_watcher(files_state.clone());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(accounts_state)
        .manage(files_state)
        .invoke_handler(tauri::generate_handler![
            create_vault,
            open_vault,
            lock_vault,
            is_unlocked,
            list_entries,
            search_entries,
            add_entry,
            update_entry,
            delete_entry,
            export_backup,
            export_csv,
            import_csv,
            import_kdbx,
            generate_password,
            copy_to_clipboard_with_autoclear,
            create_file_vault,
            open_file_vault,
            lock_file_vault,
            is_file_vault_unlocked,
            list_files,
            add_files,
            extract_file,
            delete_file,
            export_file_vault_backup,
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
