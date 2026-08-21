# SecureVault

A portable, fully offline password and sensitive-data manager. Data lives
encrypted on a removable drive (SSD/USB); pull the drive and nothing usable
is left on the host machine. The dashboard aims for the everyday
familiarity of a spreadsheet, not the friction of a traditional password
manager form.

See `TZ_SecureVault.md` for the full specification this project is being
built against, `THREAT_MODEL.md` for what SecureVault does and does not
protect against, and `SECURITY.md` for the security design principles and
audit requirements.

## Status

- [x] `vault-core`: the encryption core (Argon2id KDF + SQLCipher-backed
      entry store), fully unit/integration tested, zero UI dependencies.
- [ ] Tauri + React dashboard (blocked in this environment — see below).
- [ ] Local-only server, portable packaging, backup UX, CI.

## Why the crypto core came first

Per the spec's own instructions to the implementing agent: no custom
cryptographic primitives, ever — only vetted libraries — and the crypto
core gets tests before anything else gets built on top of it. Concretely:

- **KDF**: Argon2id via RustCrypto's `argon2` crate, tuned to 256 MiB / 3
  passes / 4-way parallelism by default (`Argon2Params::standard()`).
- **Storage**: SQLCipher (AES-256, per-page HMAC-authenticated) via
  `rusqlite`'s `bundled-sqlcipher` feature — no separate container format
  (VeraCrypt-style) is implemented; SQLCipher's own encrypted file *is* the
  container. This was a deliberate simplification from the original
  container-format options in the spec (see `TZ_SecureVault.md` §3): a
  from-scratch VeraCrypt-compatible container is large surface area for no
  real benefit here.
- The master password itself never touches disk; only the derived key does,
  held in memory in a type that zeroizes on drop.

Run the crypto core's tests:

```sh
cargo test --workspace
```

## Frontend: currently blocked

This environment's `npm`/Node networking cannot reach `registry.npmjs.org`
(connection reset on every request, including from a bare Node `https` call
— `curl` to the same host works fine, so this looks like a Node-specific
networking quirk in this sandbox, not a real firewall). `cargo`/crates.io
access is unaffected. Until that's resolved (or the Tauri+React scaffold is
generated in an environment with working `npm`), the dashboard work is on
hold.

## Layout

```
Cargo.toml              workspace root
crates/vault-core/      encryption + storage core (this is what's built so far)
TZ_SecureVault.md        original specification
THREAT_MODEL.md
SECURITY.md
```
