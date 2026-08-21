# Threat Model

CIPHERDEN stores passwords and other sensitive records, encrypted at rest,
on a removable drive (SSD/USB). This document states plainly what that
protects against and what it does not, so users can make an informed
decision about how to rely on it.

## What we protect against

- **Theft or loss of the physical drive.** Without the master password, the
  data on it is useless. The vault file is encrypted with SQLCipher
  (AES-256), keyed by a 256-bit key derived from your master password via
  Argon2id.
- **Compromise of the host computer *after* the drive is removed.** The
  application keeps decrypted data only in memory while the vault is open.
  When the vault is locked or the application closes, no plaintext is left
  on the host's disk.
- **Brute-forcing the master password.** Argon2id with deliberately heavy
  memory/time parameters (256 MiB, 3 passes by default) makes offline
  brute-force and GPU/ASIC cracking expensive.
- **Network attacks.** The application never opens a port other than a
  loopback (`127.0.0.1`) listener used by its own local UI. It has no cloud
  sync and makes no outbound network calls.

## What we do NOT protect against

- **A keylogger or other active malware on the host machine at the moment
  you type your master password.** If the host is already compromised while
  you unlock the vault, the attacker can capture the password directly. No
  software running on a compromised host can fully defend against this.
- **Coercion ("rubber-hose cryptanalysis").** Being forced to reveal your
  master password is not something software can prevent. A hidden-volume /
  plausible-deniability feature is being researched for a future release
  (see ROADMAP) but is explicitly *not* promised or relied upon until it
  ships and has been reviewed — a half-working deniability feature is worse
  than none, because it creates false confidence.
- **Physical drive failure with no backup.** An SSD that fails electrically
  is generally not recoverable, even in a data-recovery lab, once its data
  is encrypted. This is why the application makes creating a second
  encrypted backup a first-class, one-click action rather than something
  left to the user to figure out — but it cannot force you to actually make
  one. See the in-app backup prompt and `SECURITY.md`.

## Trust boundaries

- The vault's metadata sidecar file (`*.meta.json`, next to the `.db` file)
  is **not secret**. It holds only the KDF salt and Argon2id parameters,
  which by design do not need to be hidden — only the master password does.
- All cryptographic primitives are provided by external, audited libraries
  (RustCrypto `argon2`, SQLCipher). CIPHERDEN does not implement its own
  encryption, key derivation, or random number generation.
