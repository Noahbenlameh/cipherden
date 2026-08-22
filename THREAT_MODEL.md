# Threat Model

CIPHERDEN stores passwords, files, and other sensitive records, encrypted
at rest, on a removable drive (SSD/USB), behind one encrypted **Shell**
holding independent, separately-password-protected **zones**. This
document states plainly what that protects against and what it does not,
so users can make an informed decision about how to rely on it.

## What we protect against

- **Theft or loss of the physical drive.** Without a working Shell
  password, nothing on the drive is usable — the Shell file (the only file
  visible on disk) is encrypted with SQLCipher (AES-256), and every zone
  inside it is separately AES-256-GCM-encrypted under its own independent,
  Argon2id-derived key.
- **Someone finding the drive without knowing what it is.** Without the
  Shell password, an attacker sees a single, generically-named encrypted
  file — not a list of zones, not their labels, not even a count. A zone's
  existence (e.g. that a "Seed phrases" zone exists at all) is hidden, not
  just its contents.
- **Losing (or being coerced into revealing) one of your two Shell
  passwords, but not the other.** The Shell has two independent, equally
  strong passwords (primary and recovery) — either unlocks it, and either
  can reset the other. This is a deliberate trade-off the user chose: real
  recoverability against forgetting one password, at the cost of there
  being no way back in if *both* are lost (see below).
- **Compromise of the host computer *after* the drive is removed.** The
  application keeps decrypted data only in memory while a zone is open, and
  each encryption key additionally lives in its own locked (`mlock`/
  `VirtualLock`) memory page so the OS is asked never to swap it to disk —
  see "Memory and swap," below, for the limits of this.
- **Brute-forcing a Shell or zone password.** Argon2id with deliberately
  heavy memory/time parameters (256 MiB, 3 passes by default) makes offline
  brute-force and GPU/ASIC cracking expensive, for every password slot.
- **A weak "reset" attack vector.** There is no password-reset mechanism an
  attacker who doesn't already know a valid password can invoke. Resetting
  a Shell password always requires already knowing a currently-valid one
  (primary or recovery) — there is no way to lock out the legitimate owner
  without also already having full access yourself.
- **Network attacks.** The application makes no network calls at all —
  Tauri's desktop IPC is not a TCP socket, so there is no port to
  accidentally expose, not even on loopback.

## What we do NOT protect against

- **A keylogger or other active malware on the host machine at the moment
  you type a Shell or zone password.** If the host is already compromised
  while you unlock something, the attacker can capture the password
  directly. No software running on a compromised host can fully defend
  against this.
- **Losing (or forgetting) *both* Shell passwords.** This is the direct
  cost of the existence-hiding design: zones have no files of their own to
  fall back to, so if neither the primary nor the recovery password is
  ever recoverable again, the Shell — and everything in it — is
  permanently unrecoverable. This was a deliberate, informed choice (the
  user explicitly preferred this over keeping zones as separately-openable
  standalone files), and the app states it plainly on every Shell-creation
  screen rather than burying it in documentation.
- **Coercion ("rubber-hose cryptanalysis").** Being forced to reveal a
  Shell password is not something software can prevent. True hidden-volume
  /plausible-deniability protection for the Shell itself (i.e. deniability
  toward someone who *knows* a Shell exists, not just hiding zones from
  someone who doesn't) remains unbuilt — see ROADMAP — and is explicitly
  *not* promised until it ships and has been reviewed; a half-working
  deniability feature is worse than none, because it creates false
  confidence.
- **Physical drive failure with no backup.** An SSD that fails electrically
  is generally not recoverable, even in a data-recovery lab, once its data
  is encrypted. This is why the application makes a full Shell backup a
  first-class, one-click action rather than something left to the user to
  figure out — but it cannot force you to actually make one.
- **A determined forensic examiner establishing that the Shell file is
  probably a CIPHERDEN container.** Existence-hiding here means hiding
  *which zones exist and what they contain* from someone without the Shell
  password — it does not (and cannot, without real steganography, which
  this project does not attempt) hide the fact that some encrypted
  container exists on the drive at all.
- **No independent security audit yet.** Every cryptographic primitive
  used (Argon2id, AES-256-GCM, SQLCipher) is vetted and widely used; the
  way this project combines them (the Shell/zone/key-slot architecture) is
  our own design and has not been reviewed by a third party. Treat this as
  appropriate for passwords and general files today; do **not** rely on it
  for your highest-value secrets (e.g. crypto seed phrases — use a hardware
  wallet as the primary option) until an audit has happened. See
  `SECURITY.md`.

## Memory and swap

Each encryption key (`VaultKey`) is allocated on its own dedicated memory
page and locked (`mlock`/`VirtualLock`) so the operating system is asked
never to write that page to swap, and it's zeroed as soon as it's no longer
needed. This is **best-effort**: some environments (containers, restrictive
`RLIMIT_MEMLOCK`) refuse the lock for an unprivileged process, and the app
still runs without it, just without this specific hardening in that
environment. This protects the *keys* specifically. The much larger
decrypted contents of an open zone (the in-memory database itself, while
you're actively using it) are **not** individually memory-locked — doing so
would require replacing SQLite's own memory allocator, a significantly
larger undertaking not yet attempted. In practice this is the same level of
exposure any password manager has while actively unlocked; it is not unique
to CIPHERDEN, but it is not fully eliminated either.

## Trust boundaries

- The Shell's sidecar file (`*.meta.json`, next to the Shell's `.vault`
  file) is **not secret**. It holds the KDF salts and Argon2id parameters
  for both password slots, which by design do not need to be hidden — only
  the passwords themselves do.
- All cryptographic primitives are provided by external, audited libraries
  (RustCrypto `argon2`, `aes-gcm`; SQLCipher). CIPHERDEN does not implement
  its own encryption, key derivation, or random number generation.
