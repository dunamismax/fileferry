# Security Design

FileFerry is pre-v1 and the repository format is not frozen. This document
records the security decisions that format v0 implementation work must follow.
Changing these decisions requires updating this document, repository-format
fixtures when they exist, and the focused crypto tests.

## Goals

- Encrypt file contents, file names, directory structure, snapshot metadata,
  indexes, and sensitive repository policy/config objects before storage.
- Authenticate every encrypted object.
- Fail closed for wrong passphrases, wrong keys, tampering, truncation, swapped
  objects, replayed metadata, and malformed metadata.
- Keep plaintext repository bootstrap metadata limited, justified, and
  insufficient to reveal backup shape.
- Keep logs, diagnostics, human output, JSON, JSONL, tests, and debug output
  free of passphrases, raw key material, cloud credentials, signed URLs, bearer
  tokens, and full environment dumps.

## Current References

These references were checked before the v0 choices below:

- RFC 9106 for Argon2id. It specifies Argon2 version 0x13 and recommends
  Argon2id, including 64 MiB/t=3 for memory-constrained environments and
  2 GiB/t=1 for high-memory defaults.
- RFC 8439 for ChaCha20-Poly1305 AEAD.
- libsodium's XChaCha20-Poly1305 guidance for extended-nonce AEAD use and
  nonce uniqueness across related messages.
- RFC 5869 for HKDF extract-and-expand key derivation.

## AEAD

Format v0 uses XChaCha20-Poly1305 for object encryption.

Reasons:

- It is an authenticated encryption mode, so ciphertext modification,
  truncation, wrong-key reads, and wrong authenticated data fail during decrypt.
- The 192-bit nonce gives a wide random nonce space for immutable repository
  objects and avoids relying on a mutable central counter.
- It is available through maintained RustCrypto crates without shelling out to
  OpenSSL or platform tools.

Rules:

- Generate a fresh random nonce for every encryption under a given subkey.
- Store the nonce next to the ciphertext. The nonce is not secret.
- Treat any AEAD open failure as authentication failure. Do not return partial
  plaintext.
- Never reuse a nonce with the same subkey.
- Do not add unauthenticated compression, framing, or metadata around
  ciphertext that affects restore behavior.

## Key Hierarchy

Each repository has a random 256-bit master key. User passphrases and future
key-file unlock methods decrypt that master key; they are not used directly for
repository objects.

Format v0 derives subkeys from the master key with HKDF-SHA-256:

```text
HKDF salt = "fileferry\0format-v0\0hkdf\0"
info      = "fileferry\0subkey\0" || purpose || len(context) || context
output    = 32 bytes
```

Initial subkey purposes:

- `chunk-identity`
- `chunk-data`
- `snapshot-metadata`
- `index`
- `policy-config`
- `upload-state`
- `prune-mark`
- `lease-state`

The `context` must bind a subkey to the repository identity or a narrower
operation context. Domain labels must not be reused for incompatible object
types.

## Passphrase KDF

Format v0 uses Argon2id version 0x13 for passphrase unlock.

Default parameters:

```text
memory_cost = 65536 KiB
time_cost   = 3
parallelism = 4
salt        = 16 random bytes per key slot
output      = 32 bytes
```

These defaults follow RFC 9106's memory-constrained recommendation. A
high-memory profile may use 2 GiB memory, time cost 1, and parallelism 4 after
unlock latency has been measured on target hardware.

The KDF parameters are plaintext bootstrap metadata because unlock requires
them before the master key is available. They are authenticated as associated
data when decrypting the wrapped master key.

KDF migration is per key slot. Existing key slots remain readable with the
parameters stored in the slot. Raising defaults creates new slots with the new
parameters and can retire older slots after the user proves another unlock path
works. Repository object encryption does not change when only KDF parameters
change because object keys derive from the unchanged repository master key.

## Key Slots And Unlock

A key slot contains:

- KDF algorithm and parameters.
- KDF salt.
- AEAD nonce.
- XChaCha20-Poly1305 ciphertext containing the repository master key.

The key-slot AEAD associated data is:

```text
"fileferry\0format-v0\0key-slot-wrap\0"
|| kdf_algorithm
|| memory_cost
|| time_cost
|| parallelism
|| len(salt)
|| salt
```

Unlock flow:

1. Read the plaintext key-slot metadata.
2. Derive the wrapping key with Argon2id.
3. Decrypt the wrapped master key with the key-slot associated data.
4. Fail closed if derivation fails, authentication fails, or the plaintext is
   not exactly 32 bytes.

## Backup Lease Coordination

`ferry backup` for initialized local and S3-compatible repositories now
acquires encrypted `locks/<lease-id>` command lease state before snapshot
publication. Before writing chunk, index, manifest, or commit objects, current
code lists `locks/`, authenticates and validates readable lease state, rejects
another active readable lease as a locked repository, ignores expired readable
leases, writes its own encrypted backup lease, and rechecks active leases after
the write. After the snapshot write path returns, it best-effort deletes its
own lease.

This is narrow command-level coordination. It does not implement stale-lease
repair, upload-state recovery, or a final concurrent-backup safety claim.

## Key Add

`ferry key add` adds one new passphrase unlock path for an initialized local
or S3-compatible repository. The command must first unlock the repository with
the existing passphrase from `FILEFERRY_PASSWORD` or
`FILEFERRY_PASSWORD_FILE`. The new passphrase comes from
`--new-password-file`, `FILEFERRY_NEW_PASSWORD`, or
`FILEFERRY_NEW_PASSWORD_FILE`.

The command creates a new key slot that wraps the existing repository master
key. It writes that slot as an immutable `key-slots/<key-slot-id>` object and
does not rewrite the original bootstrap key slot.

Before writing the new key-slot object, current code acquires encrypted
`locks/<lease-id>` command lease state. Another active readable lease rejects
the command as a locked repository, and malformed lease state fails closed
before the key-slot object is written.

`key add` does not create a new repository master key, does not re-encrypt or
rewrite chunks, manifests, indexes, snapshot commit markers, forget markers,
or policy/config objects, and does not recover a lost master key. If no
existing unlock path works, the command fails closed without writing a new key
slot.

## Key Remove

`ferry key remove <KEY_SLOT_ID>` removes one externally added passphrase
unlock path for an initialized local or S3-compatible repository. The command
must first unlock the repository with `FILEFERRY_PASSWORD` or
`FILEFERRY_PASSWORD_FILE`.

Removal is marker-based. The command writes an immutable
`key-slot-removals/<key-slot-id>` marker and does not delete the original
`key-slots/<key-slot-id>` object. The marker carries a keyed removal check
derived from the repository master key, repository id, and key-slot id, so a
marker is accepted only after the candidate slot decrypts to the repository
master key and the marker check verifies.

This slice removes only externally added key slots. The original bootstrap key
slot is not removable. Before writing a removal marker, the command proves
that the supplied current passphrase unlocks a remaining non-removed slot. If
the supplied passphrase only proves the slot being removed, the command fails
closed without writing removal state.

Before writing the removal marker, current code acquires encrypted
`locks/<lease-id>` command lease state. Another active readable lease rejects
the command as a locked repository, and malformed lease state fails closed
before the key-slot removal marker is written.

`key remove` does not create a new repository master key, does not re-encrypt
or rewrite chunks, manifests, indexes, snapshot commit markers, forget
markers, policy/config objects, or the original bootstrap slot, and does not
recover lost keys.

## Recovery Export

`ferry key export-recovery --output <FILE>` is implemented for initialized
local repositories. It unlocks the repository with `FILEFERRY_PASSWORD` or
`FILEFERRY_PASSWORD_FILE`, creates a standalone encrypted recovery package,
and writes that package to a destination file that must not already exist.

The current recovery package is password-protected by the same current
repository passphrase used to run the command. It is intended as an encrypted,
offline copy of unlock material for future recovery-import work, not as a
plaintext key export and not as recovery from a lost passphrase.

The recovery export format records:

- `schema_version`, `magic`, `format_version`, and `export_type`.
- Repository id and a random export id.
- Creation time as Unix seconds.
- Warning text stating that the export should be stored separately from the
  repository and protected like a backup key.
- KDF parameters and salt for Argon2id v1.3.
- AEAD algorithm `xchacha20_poly1305`, AEAD nonce, and encrypted master-key
  bytes.
- A keyed master-key check derived from the repository master key, repository
  id, and export id so future import can reject exports for a different
  master key.

The recovery-export AEAD associated data is:

```text
"fileferry\0format-v0\0recovery-export-wrap\0"
little-endian format version
KDF algorithm id
little-endian KDF memory cost, time cost, and parallelism
length-prefixed KDF salt
length-prefixed repository id
```

The command never prints raw master keys, recovery plaintext, passphrases, or
repository URLs with credentials in human output, JSON, JSONL, logs, errors,
or debug output. Output reports the repository id, export id, redacted
destination, KDF summary, AEAD algorithm, warning text,
`recovery_import_implemented: false`, `raw_master_key_exported: false`, and
`reencrypted_repository_objects: false`.

Recovery import is not implemented.

Current format-fixture coverage includes one standalone encrypted recovery
export package. Focused tests verify that current code can parse and authenticate
the package with the fixture passphrase, rejects malformed recovery-export JSON,
rejects unsupported recovery-export format versions, rejects wrapped-master-key
ciphertext tampering as an unlock failure, and rejects a tampered
`master_key_check` as an invalid recovery export.

Current committed snapshot-data fixture coverage includes one initialized
repository with a plaintext commit marker, encrypted manifest, encrypted index,
and encrypted chunks. Focused tests prove that current code authenticates and
validates those bytes through read, check, and restore paths; rejects malformed
commit JSON, encrypted-object framing, and decrypted manifest metadata; rejects
manifest, index, and chunk ciphertext tampering; rejects wrong object names and
kinds through AEAD context binding; rejects manifest and index metadata identity
mismatches; and rejects unsupported commit, manifest, and index schema versions.

Current forget/prune-state fixture coverage includes one initialized repository
after a prune sweep, plus the captured plaintext forget marker that the sweep
deleted. Focused tests prove that current code reads the forget marker,
authenticates and validates encrypted prune plan and completion state, checks
the retained post-prune snapshot data, rejects malformed forget-marker JSON,
rejects forget-marker schema and metadata identity mismatches, rejects malformed
prune encrypted-object framing and decrypted metadata, rejects prune plan and
completion ciphertext tampering, rejects wrong object names and authenticated
kinds through AEAD context binding, rejects prune metadata identity mismatches,
rejects unsupported prune schemas and format versions, rejects tampered
completion state during prune recovery scanning, and rejects stale pending
prune-plan replay when current commit/forget marker state no longer matches the
marked plan.

Current policy/config fixture coverage includes one initialized repository with
one encrypted policy/config object. Focused tests prove that current code
authenticates and validates the fixture, rejects malformed encrypted framing and
decrypted metadata, rejects policy ciphertext tampering, rejects wrong object
names and authenticated kinds through AEAD context binding, rejects policy
metadata identity mismatches, rejects unsupported policy/config schema and
format versions, rejects repository identity mismatches, rejects invalid
retention shape, and treats an already-present matching policy body as an
idempotent write result.

Current upload-state fixture coverage includes one initialized repository with
one encrypted upload-state object. Focused tests prove that current code
authenticates and validates the fixture, rejects malformed encrypted framing and
decrypted metadata, rejects upload-state ciphertext tampering, rejects wrong
object names and authenticated kinds through AEAD context binding, rejects
upload-state metadata identity mismatches, rejects unsupported upload-state
schema and format versions, rejects repository identity mismatches, rejects
stale upload-state replay when current commit/forget marker state no longer
matches the marked state, and treats an already-present matching upload state as
an idempotent write result.

Current migration-detection fixture coverage includes bootstrap-only fixtures
for current v0, an unsupported future format version, unsupported v0 feature
flags, and unversioned pre-v0 metadata. Focused tests prove that current code
can inspect bootstrap format compatibility without unlocking key slots, rejects
malformed bootstrap JSON during inspection, rejects future format versions and
unknown current-format feature flags before unlock, and rejects unversioned
pre-v0 metadata instead of guessing a migration.

Current lease-state fixture coverage includes one initialized repository with
one encrypted `locks/<lease-id>` object. Focused tests prove that current code
authenticates and validates the fixture, rejects malformed encrypted framing and
decrypted metadata, rejects lease-state ciphertext tampering, rejects wrong
object names and authenticated kinds through AEAD context binding, rejects lease
metadata identity mismatches, rejects unsupported lease-state schema and format
versions, rejects repository identity mismatches, rejects invalid lease
expiration windows, rejects expired leases for active use, and treats an
already-present matching lease state as an idempotent write result. Non-dry-run
forget, non-dry-run prune, and key-management mutation paths now use encrypted
lease state for best-effort command coordination: active readable leases reject
the command as a locked repository, expired readable leases are ignored,
malformed lease objects fail closed before forget writes markers, prune deletes
candidates, or key-management writes key-slot mutation objects, and each
command best-effort releases its own lease after the mutation path returns.
Other command-level lease enforcement is not implemented yet.

Current fixture-covered repository JSON schemas are strict for the current v0
objects they cover. Focused tests prove that unknown fields in plaintext
objects, encrypted-object frames, and decrypted repository metadata are rejected
as decode failures before validation or use. The bootstrap inspection gate is
the exception: it intentionally reads only the small compatibility envelope
needed to classify future formats or feature flags before unlock.

## Key Rotation

Key rotation has two different meanings:

- Unlock rotation changes key slots, passphrases, key files, or recovery
  packages that wrap the same repository master key.
- Repository rekey creates a new master key and rewrites or re-encrypts all
  repository objects.

Format v0 `key rotate` means unlock rotation. For initialized local and
S3-compatible repositories, `ferry key rotate --retire-key-slot
<KEY_SLOT_ID>...` unlocks the repository with `FILEFERRY_PASSWORD` or
`FILEFERRY_PASSWORD_FILE`, writes one new immutable
`key-slots/<key-slot-id>` object from
`--new-password-file`, `FILEFERRY_NEW_PASSWORD`, or
`FILEFERRY_NEW_PASSWORD_FILE`, proves that the new slot unlocks the existing
repository master key, then writes immutable removal markers for the
explicitly selected externally added key slots.

Before writing the new key-slot object or removal markers, current code
acquires encrypted `locks/<lease-id>` command lease state. Another active
readable lease rejects the command as a locked repository, and malformed lease
state fails closed before the new key-slot object is written.

`key rotate` does not create a new repository master key, does not re-encrypt
or rewrite chunks, manifests, indexes, snapshot commit markers, forget
markers, policy/config objects, or the original bootstrap slot, does not
delete `key-slots/<key-slot-id>` objects, does not remove unselected key
slots, and does not recover lost keys.

## Tamper And Corruption Errors

Library crates should keep these classes distinguishable enough for stable CLI
exit-code mapping:

- `wrong_password`: KDF completed but key-slot authentication failed.
- `wrong_key`: object authentication failed with the supplied repository key.
- `corrupt_object`: ciphertext/tag/nonce/framing is malformed or truncated.
- `context_mismatch`: authenticated object context does not match the object
  being opened.
- `unsupported_format`: bootstrap version or algorithm is unknown.
- `malformed_metadata`: plaintext or decrypted metadata is not valid for its
  schema, including unknown fields in a fixture-covered current-v0 object.
- `replayed_metadata`: authenticated metadata is valid but older than the
  required repository state.

The CLI maps authentication failures to exit code 4 and integrity/tamper
failures to exit code 6 when it has enough context to distinguish them. When it
cannot distinguish wrong credentials from tampering without leaking information,
it should use the more conservative authentication/integrity message and keep
machine fields structured.

JSON and JSONL failures should include the normal CLI envelope plus a structured
error object:

```json
{
  "code": "corrupt_object",
  "exit_code": 6,
  "message": "repository object failed authentication",
  "object_kind": "index",
  "object_name": "objects/index/ab/example",
  "recoverable": false
}
```

`message` is for display and may change before v1. `code`, `exit_code`,
`object_kind`, `object_name`, and `recoverable` are the planned stable fields.
Sensitive plaintext metadata must not be placed in any error field.

## Implemented Evidence

The `fileferry-crypto` crate currently includes focused tests for:

- Master key creation and passphrase unlock.
- Wrong passphrase failure.
- Tampered key-slot failure.
- Object encryption/decryption.
- Wrong subkey failure.
- Bit-flipped ciphertext failure.
- Truncated ciphertext failure.
- Core backup pipeline chunk/index/manifest writes that keep source paths,
  tags, and directory shape inside encrypted metadata objects and use keyed
  chunk identities for object placement.
- Wrong authenticated object context failure.
- Authenticated snapshot-manifest and chunk-index reads.
- Committed snapshot-data fixture read, check, and selected-file restore.
- Wrong repository key failure for encrypted repository metadata.
- Bit-flipped and truncated repository object read failures.
- Swapped repository object failures across realistic object names.
- Replayed chunk-index metadata identity failures.
- Malformed decrypted metadata failures.
- Redacted `Debug` output for master keys.

The broader adversarial test matrix still needs format migration failures once
format fixtures and migrations exist.
