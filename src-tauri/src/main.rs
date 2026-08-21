// SecureVault desktop shell. All cryptography and storage logic lives in
// `vault-core`; this crate only wires that core to Tauri IPC commands and
// enforces the app-level policies the spec requires (auto-lock, clipboard
// auto-clear, no plaintext left behind).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rand::Rng;
use serde::Serialize;
use tauri::State;
use vault_core::store::{Entry, NewEntry};
use vault_core::{Argon2Params, Vault};

const AUTO_LOCK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CLIPBOARD_CLEAR_DELAY: Duration = Duration::from_secs(20);
const LOCK_CHECK_INTERVAL: Duration = Duration::from_secs(1);

struct VaultState {
    vault: Option<Vault>,
    last_activity: Instant,
}

impl VaultState {
    fn new() -> Self {
        Self {
            vault: None,
            last_activity: Instant::now(),
        }
    }

    fn touch(&mut self) {
        self.last_activity = Instant::now();
    }
}

type AppState = Arc<Mutex<VaultState>>;

fn to_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
fn create_vault(state: State<'_, AppState>, path: String, password: String) -> Result<(), String> {
    let vault =
        Vault::create(PathBuf::from(path), &password, Argon2Params::standard()).map_err(to_err)?;
    let mut guard = state.lock().map_err(to_err)?;
    guard.vault = Some(vault);
    guard.touch();
    Ok(())
}

#[tauri::command]
fn open_vault(state: State<'_, AppState>, path: String, password: String) -> Result<(), String> {
    let vault = Vault::open(PathBuf::from(path), &password).map_err(to_err)?;
    let mut guard = state.lock().map_err(to_err)?;
    guard.vault = Some(vault);
    guard.touch();
    Ok(())
}

#[tauri::command]
fn lock_vault(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.vault = None; // dropping the Vault drops the SQLCipher connection and zeroizes the key
    Ok(())
}

#[tauri::command]
fn is_unlocked(state: State<'_, AppState>) -> Result<bool, String> {
    let guard = state.lock().map_err(to_err)?;
    Ok(guard.vault.is_some())
}

#[tauri::command]
fn list_entries(state: State<'_, AppState>) -> Result<Vec<Entry>, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.vault.as_ref().ok_or("vault is locked")?;
    vault.list_entries().map_err(to_err)
}

#[tauri::command]
fn search_entries(state: State<'_, AppState>, query: String) -> Result<Vec<Entry>, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.vault.as_ref().ok_or("vault is locked")?;
    vault.search(&query).map_err(to_err)
}

#[tauri::command]
fn add_entry(state: State<'_, AppState>, entry: NewEntry) -> Result<i64, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.vault.as_ref().ok_or("vault is locked")?;
    vault.add_entry(&entry).map_err(to_err)
}

#[tauri::command]
fn update_entry(state: State<'_, AppState>, id: i64, entry: NewEntry) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.vault.as_ref().ok_or("vault is locked")?;
    vault.update_entry(id, &entry).map_err(to_err)
}

#[tauri::command]
fn delete_entry(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.vault.as_ref().ok_or("vault is locked")?;
    vault.delete_entry(id).map_err(to_err)
}

#[tauri::command]
fn export_backup(state: State<'_, AppState>, dest_dir: String) -> Result<String, String> {
    let mut guard = state.lock().map_err(to_err)?;
    guard.touch();
    let vault = guard.vault.as_ref().ok_or("vault is locked")?;
    vault
        .export_backup(PathBuf::from(dest_dir))
        .map(|p| p.display().to_string())
        .map_err(to_err)
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

fn main() {
    let state: AppState = Arc::new(Mutex::new(VaultState::new()));
    let lock_watcher_state = state.clone();

    std::thread::spawn(move || loop {
        std::thread::sleep(LOCK_CHECK_INTERVAL);
        if let Ok(mut guard) = lock_watcher_state.lock() {
            if guard.vault.is_some() && guard.last_activity.elapsed() > AUTO_LOCK_TIMEOUT {
                guard.vault = None;
            }
        }
    });

    tauri::Builder::default()
        .manage(state)
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
            generate_password,
            copy_to_clipboard_with_autoclear,
        ])
        .setup(|app| {
            // Loopback-only by construction: Tauri's webview talks to this
            // process over an internal IPC channel, not a TCP socket, so
            // there is no port to accidentally expose beyond localhost.
            let _ = app.handle();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running SecureVault");
}
