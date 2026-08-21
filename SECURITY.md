# Security Policy

## Design principles

1. **No home-grown cryptography.** Every cryptographic primitive comes from
   an established, maintained library:
   - Key derivation: Argon2id via the RustCrypto [`argon2`](https://crates.io/crates/argon2) crate.
   - Encryption at rest: [SQLCipher](https://www.zetetic.net/sqlcipher/) (AES-256, authenticated per-page via HMAC), via `rusqlite`'s bundled build.
   - Randomness: the operating system CSPRNG via the [`rand`](https://crates.io/crates/rand) crate.

   If a needed capability isn't available in a vetted library, the correct
   response is to stop and discuss with a human, not to write a custom
   implementation. This applies to AI coding agents working on this
   repository as much as to human contributors.

2. **Local-only by default.** The application's local web server binds only
   to `127.0.0.1`. It must never listen on `0.0.0.0` or any other interface
   unless a future, explicitly-opt-in feature says otherwise, and any such
   feature requires a security review before merging.

3. **No telemetry.** The application makes no outbound network calls of any
   kind in its default configuration.

4. **Master password never touches disk.** Only the Argon2id-derived key is
   used, and only in memory. It is wrapped in a type that zeroizes on drop.

See `THREAT_MODEL.md` for what this protects against and what it explicitly
does not.

## Reporting a vulnerability

This project has not yet had its first release or independent security
audit. Until a formal disclosure channel is set up, please open a GitHub
issue marked `security` with as much detail as you can share, or contact the
maintainer directly if the issue is sensitive enough that a public issue
would be irresponsible.

## Pre-commercial audit requirement

Per the project roadmap, an independent third-party security audit is a
hard requirement before any commercial release — not an optional milestone.
No commercial release should ship without one.
