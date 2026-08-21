# CIPHERDEN

*(formerly "SecureVault" during early development — same project, new name)*

A portable, fully offline password and sensitive-data manager. Data lives
encrypted on a removable drive (SSD/USB); pull the drive and nothing usable
is left on the host machine. The dashboard aims for the everyday
familiarity of a spreadsheet, not the friction of a traditional password
manager form.

See `TZ_SecureVault.md` for the original specification this project started
from, `THREAT_MODEL.md` for what CIPHERDEN does and does not protect
against, and `SECURITY.md` for the security design principles and audit
requirements.

**Start here for full context:** [`PROJECT_MAP.md`](PROJECT_MAP.md) —
architecture, every decision's rationale, current status, and a detailed
usage walkthrough. Kept current with the code; read it before making
changes, whether you're a human or an AI agent picking this project back up.

## Status

MVP (spec §4.1) is complete and working:

- [x] `vault-core`: Argon2id KDF + SQLCipher-backed entry store (create/
      open/CRUD/search/backup), zero UI dependencies, fully tested.
- [x] Tauri desktop shell + hand-written HTML/CSS/JS dashboard (table view,
      search, category filter, password generator, clipboard auto-clear,
      auto-lock).
- [x] Portable binary — verified to run standalone from any directory with
      no files alongside it (frontend is embedded in the binary).
- [x] Encrypted backup export.
- [x] Import from CSV (e.g. a Google Sheets export) and KeePass `.kdbx`.
- [x] Export selected rows to CSV.
- [x] CI (GitHub Actions): fmt/clippy/build/test on macOS, Linux, Windows.
- [x] Cyberpunk/HUD visual redesign.
- [x] Native OS file/folder pickers everywhere a path used to be typed by
      hand (`tauri-plugin-dialog`, itself just a Cargo dependency — no npm).
- [x] Second zone: **Files** — an encrypted file safe (add/extract/delete,
      independent password from the Accounts zone), reachable via the zone
      tabs in the dashboard.

Next up: more zones as the user requests them (seed phrases, etc.) — see
**Vision** below. SSH/Tailscale launcher explicitly parked for now.

## Why the crypto core came first

Per the original spec's instructions to the implementing agent: no custom
cryptographic primitives, ever — only vetted libraries — and the crypto core
gets tests before anything else gets built on top of it. Concretely:

- **KDF**: Argon2id via RustCrypto's `argon2` crate, tuned to 256 MiB / 3
  passes / 4-way parallelism by default (`Argon2Params::standard()`).
- **Storage**: SQLCipher (AES-256, per-page HMAC-authenticated) via
  `rusqlite`'s `bundled-sqlcipher-vendored-openssl` feature — no separate
  container format (VeraCrypt-style) is implemented; SQLCipher's own
  encrypted file *is* the container, and it bundles its own OpenSSL so it
  never depends on whatever (if anything) is installed on the host.
- **Import/export**: CSV via the `csv` crate; KeePass `.kdbx` via the
  `keepass` crate. Both only ever read/write already-vetted formats.
- The master password itself never touches disk; only the derived key does,
  held in memory in a type that zeroizes on drop.

Run the tests:

```sh
cargo test --workspace
```

Run the app:

```sh
cargo build --workspace && ./target/debug/cipherden
```

## Frontend: hand-written, no bundler

This environment's `npm`/Node networking cannot reach `registry.npmjs.org`
(connection reset on every request — `cargo`/crates.io is unaffected, so
this looks like a Node-specific quirk in this sandbox rather than a real
firewall). Rather than block on that, the dashboard (`dist/`) is plain
HTML/CSS/JS with no build step, served by Tauri directly. This turned out to
be a good fit for the project's own goals anyway: fewer dependencies, and
Tauri embeds it straight into the compiled binary, which is exactly the
"one portable file" the spec asks for.

## Vision: beyond a password manager

The plan is for CIPHERDEN to grow from a single vault into a small set of
independent, equally-encrypted **zones** living side by side on the same
drive — a personal, portable security toolkit rather than a single-purpose
app. Concretely, planned zones:

- **Accounts** (done) — logins, passwords, notes.
- **Files** (done) — an encrypted file safe, added/extracted through the
  app's own UI via native OS file pickers (no OS-level mount, so it stays
  driver-free and just as portable as the accounts vault). Each file is
  stored as a blob in its own SQLCipher container (`files.vault`) —
  comfortable for documents/photos; very large files (many GB) would want a
  streaming approach instead, not attempted yet.
- **Seed phrases** — a separate, explicitly-labeled zone with its own
  warnings; hardware wallets are still recommended as the primary option.
  Not started.
- SSH/Tailscale launcher and disk-cloning/network scripts: deliberately
  parked, not planned right now — noted here only as context for the
  "zones" architecture, not a commitment.
- Further out, more speculative: eventually AI-assisted features.

Shared implementation detail: `crates/vault-core/src/container.rs` holds
the create/open/keying logic every zone reuses, so a fix or audit finding
there covers all zones at once, not one at a time.

**Architecture decision:** zones are separate encrypted container files
(like today's `.db` + `.meta.json` pair) sitting on one ordinary exFAT
partition — not separate raw disk partitions. exFAT is the one filesystem
that's natively readable/writable on Windows, macOS, and Linux with zero
drivers; carving up the drive into real partitions would require installing
mount drivers on every host machine, which breaks the "plug in and it just
works" promise this project is built around.

Hidden-volume-style duress protection and hardware second-factor (YubiKey)
remain explicitly deferred — see `THREAT_MODEL.md` — until they can be
built and verified properly rather than half-implemented.

## Layout

```
Cargo.toml              workspace root
crates/vault-core/      encryption + storage core (Argon2id, SQLCipher, import/export)
src-tauri/              Tauri desktop shell (IPC commands, auto-lock, clipboard policy)
dist/                   hand-written frontend (no bundler)
TZ_SecureVault.md       original specification
THREAT_MODEL.md
SECURITY.md
```
