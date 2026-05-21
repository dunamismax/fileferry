# BUILD.md

Active build plan for FileFerry and its `ferry` command.

`README.md` explains the product. `AGENTS.md` holds durable repo operating
rules. This file tracks current state, implementation phases, release scope,
and verification.

Treat unchecked boxes as plan. Move stable material into `docs/`, `README.md`,
or runbooks as the implementation matures.

Last reviewed: 2026-05-21.

---

## Current Baseline

- Repository exists with MIT license.
- The canonical GitHub remote is
  `https://github.com/dunamismax/fileferry.git`, though local checkouts may
  fetch or push through SSH aliases and may push to more than one URL.
- Rust workspace exists with the target crate boundaries, `fileferry-cli`
  binary, `fileferry-web` homepage binary, `just` verification recipes, and
  GitHub Actions CI.
- `ferry version` supports human, JSON, and JSONL output.
- `ferry completion <SHELL>` generates shell completion scripts.
- `ferry init` creates encrypted local filesystem repositories from
  `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, and encrypted
  S3-compatible repository bootstraps from `s3://bucket[/prefix]` URLs plus
  explicit S3 endpoint, region, and credential environment variables.
- CLI repository resolution uses one shared local/S3 target parser before
  repository command execution. S3-compatible `init`, `backup`, `snapshots`,
  `ls`, `restore`, `check`, `forget`, `prune`, and key-management paths use
  the shared S3 store resolver and explicit S3 endpoint, region, and
  credential environment variables.
- S3-compatible data-path provider evidence has passed against the private
  Backblaze B2 development bucket under an isolated `fileferry/dev/...` test
  prefix. The live drill confirmed `init`, `backup`, `snapshots`, `ls`,
  `restore`, and `check` against an initialized S3-compatible repository,
  safe JSON stdout/stderr separation, restored bytes, full check success, and
  missing referenced object failure during `check` with integrity exit code
  `6`, `repository_check_missing_object`, and machine-readable object-key
  context.
- `ferry backup` opens initialized local and S3-compatible repositories,
  unlocks them with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`,
  writes an encrypted best-effort command lease before snapshot object
  publication, rejects another active readable lease as a locked repository,
  ignores expired readable leases, fails closed on malformed lease state before
  snapshot writes, creates encrypted, compressed, deduplicated snapshots
  through the core backup pipeline, best-effort releases its own lease after
  the snapshot write path returns, and exposes tested human, JSON, and
  JSONL-safe output paths.
- `ferry snapshots` and `ferry ls` open initialized local and S3-compatible
  repositories, authenticate committed encrypted manifests, and expose tested
  human, JSON, and JSONL-safe output paths.
- `ferry restore` opens initialized local and S3-compatible repositories,
  unlocks with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, selects
  snapshots by latest, snapshot id, or tag, restores all or path-scoped
  directory entries, regular-file contents, Unix symlinks, captured modified
  timestamps for restored regular files and directories, and captured
  regular-file and directory Unix permission bits where representable through
  the core restore pipeline, verifies captured regular-file and directory Unix
  UID/GID ownership after writes without calling `chown`, enforces destination
  fail-if-exists safety unless `--overwrite` is supplied for regular files,
  preflights destination safety for selected directories, regular files, and
  symlinks before destination writes, rejects requested snapshot-relative
  restore paths that do not match manifest entries before destination writes,
  creates missing parent directories for path-scoped symlink restores after
  destination safety preflight, supports dry-run reporting including planned
  modified timestamp metadata, selected regular-file and directory
  creation/birth timestamp metadata warnings, captured Unix permission
  metadata, captured Unix ownership metadata, selected symlink timestamp
  metadata, captured Unix symlink metadata, selected reportable xattr status
  warnings when xattrs were observed or xattr capture was denied, selected ACL
  status warnings when ACLs were observed or ACL capture was denied in
  constructed or future manifests, selected file flag status warnings when
  file flags were observed or file flag capture was denied in constructed or
  future manifests, selected resource fork status warnings when resource forks
  were observed or resource fork capture was denied in constructed or future
  manifests, selected Windows attribute status warnings when Windows
  attributes were observed or Windows attribute capture was denied in
  constructed or future manifests, selected sparse extent status warnings when
  sparse extents were observed or sparse extent capture was denied in
  constructed or future manifests, and metadata planning warnings, returns
  partial-success exit code `10` when metadata warnings are produced, and
  exposes tested human, JSON, and JSONL-safe output paths. Authenticated manifests with
  invalid entry topology are rejected as integrity failures before restore
  destination writes.
- `ferry check` opens initialized local and S3-compatible repositories,
  unlocks with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`,
  authenticates committed manifests and chunk indexes, reads/decompresses
  every referenced chunk, and verifies keyed chunk identities. It also accepts
  `--read-data-subset <N|PERCENT>` for deterministic count-based or
  percentage-based referenced-chunk subsets after committed metadata has been
  authenticated and validated. Runtime check failures in JSON and JSONL modes
  emit stable machine-readable failure envelopes with object keys, including
  encrypted-object authentication failures, and `CheckFinding`-shaped
  integrity details where the failing core error carries enough context.
  Missing objects referenced by committed repository metadata are reported as
  integrity failures instead of uninitialized-repository failures.
  Manifest/index chunk-reference mismatches and chunk decompression failures
  now retain snapshot-relative path, snapshot id, and object-key context where
  committed metadata provides it. Invalid decrypted manifest entry paths,
  duplicate entry paths, non-file chunk references, regular-file
  size/chunk-length mismatches, and non-directory ancestors are reported as
  integrity failures with snapshot id, object key, and path context where
  available. Metadata identity mismatches retain the repository object key in
  CLI machine-readable failure output.
- `ferry forget` opens initialized local and S3-compatible repositories,
  unlocks with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`,
  authenticates currently visible committed encrypted manifests, evaluates
  `fileferry-policy` keep rules, supports dry-run, and writes immutable
  snapshot forget markers only when not in dry-run. Non-dry-run forget writes
  and verifies an encrypted best-effort command lease before marker writes,
  rejects another active readable lease as a locked repository, ignores
  expired readable leases, fails closed on malformed lease state before
  marker writes, and best-effort releases its own lease after marker writes
  return. It does not delete chunks, manifests, indexes, or commit objects;
  storage reclamation is handled by the separate `ferry prune` command.
  JSON and JSONL output report candidate snapshots, kept snapshots, forgotten
  snapshots, item-level reasons, dry-run status, marker objects written, and
  explicit `object_deletion: false`.
- `ferry prune` opens initialized local and S3-compatible repositories,
  unlocks with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, computes
  objects reachable from forgotten snapshots and not reachable from
  non-forgotten committed snapshots, supports dry-run, writes and verifies an
  encrypted best-effort command lease before non-dry-run plan marking or
  sweeping, rejects another active readable lease as a locked repository,
  ignores expired readable leases, fails closed on malformed lease state before
  candidate deletion, writes encrypted durable prune plan state before
  sweeping, resumes incomplete sweeps when commit/forget marker state still
  matches the marked plan, and writes encrypted completion state after sweep.
  It deletes only planned commit markers, forget markers, manifests, indexes,
  and chunks; it never deletes bootstrap, key slots, key-slot removal markers,
  policy/config objects, upload state, lease state, prune state, unknown
  objects, or non-forgotten snapshot data.
  JSON and JSONL output report candidate, retained, deleted, and missing
  objects, byte counts where known, dry-run status, completion status, and
  recovery state. S3-compatible prune uses the same immutable encrypted
  prune state and object-store pipeline as local prune and does not require
  rename operations for correctness.
- `ferry key add` opens initialized local and S3-compatible repositories, unlocks with
  `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, and writes one immutable
  additional passphrase key-slot object from `--new-password-file`,
  `FILEFERRY_NEW_PASSWORD`, or `FILEFERRY_NEW_PASSWORD_FILE`. It does not
  create a new master key, rewrite the original bootstrap slot, rewrite
  chunks, manifests, indexes, commit markers, forget markers, or policy/config
  objects, and does not recover lost keys. Human, JSON, and JSONL-safe output
  report the new key-slot id, total visible key-slot count, KDF parameters,
  and `reencrypted_repository_objects: false`.
- `ferry key remove <KEY_SLOT_ID>` opens initialized local and S3-compatible
  repositories, unlocks with `FILEFERRY_PASSWORD` or
  `FILEFERRY_PASSWORD_FILE`, and writes one immutable
  `key-slot-removals/<key-slot-id>` marker for an externally added key slot
  only after the supplied current passphrase proves a remaining non-removed
  unlock path. It does not delete `key-slots/<key-slot-id>` objects, remove
  the original bootstrap key slot, create a new master key, rewrite chunks,
  manifests, indexes, commit markers, forget markers, or policy/config
  objects, re-encrypt repository objects, or recover lost keys.
  Human, JSON, and JSONL-safe output report the removed key-slot id, total
  visible key-slot count, marker object, marker creation status, and explicit
  `deleted_key_slot_objects: false` and
  `reencrypted_repository_objects: false`.
- `ferry key rotate --retire-key-slot <KEY_SLOT_ID>...` opens initialized
  local and S3-compatible repositories, unlocks with `FILEFERRY_PASSWORD` or
  `FILEFERRY_PASSWORD_FILE`, writes one immutable additional passphrase
  key-slot object from `--new-password-file`, `FILEFERRY_NEW_PASSWORD`, or
  `FILEFERRY_NEW_PASSWORD_FILE`, proves the new slot unlocks the existing
  repository master key, and writes immutable `key-slot-removals/<key-slot-id>`
  markers for explicitly selected externally added key slots. It does not
  create a new master key, remove the original bootstrap key slot, remove
  unselected key slots, delete `key-slots/<key-slot-id>` objects, rewrite or
  re-encrypt repository objects, or recover lost keys. Human, JSON, and
  JSONL-safe output report the added key-slot id, removed key-slot ids, total
  visible key-slot count, marker objects, marker creation count, KDF
  parameters, and explicit
  `deleted_key_slot_objects: false` and
  `reencrypted_repository_objects: false`.
- `ferry key export-recovery --output <FILE>` opens initialized local and
  S3-compatible repositories, unlocks with `FILEFERRY_PASSWORD` or
  `FILEFERRY_PASSWORD_FILE`, writes a standalone encrypted recovery package
  to a destination file that must not already exist, and protects that package
  with the current repository passphrase. The export records repository id,
  export id, creation time, warning text, KDF parameters, AEAD algorithm,
  nonce, encrypted master-key bytes, and a keyed master-key check for future
  verification. It does not print or store raw master-key material, implement
  recovery import, recover lost passphrases, create a new master key, rewrite
  the bootstrap slot, or rewrite or re-encrypt repository objects. Human,
  JSON, and JSONL-safe output report the repository id, export id, redacted
  destination, visible key-slot count, creation time, KDF parameters, AEAD
  algorithm, warning text, and explicit
  `recovery_import_implemented: false`, `raw_master_key_exported: false`, and
  `reencrypted_repository_objects: false`.
- CLI config discovery, profiles, environment precedence, redacted
  diagnostics, and machine-output envelopes exist for the current command
  surface.
- Format v0 security and repository-format design docs exist.
- `fileferry-crypto` has initial tested primitives for master-key creation,
  passphrase key-slot unlock, HKDF subkeys, and XChaCha20-Poly1305 object
  envelopes.
- `fileferry-storage` has a tested object-store trait, capability model,
  validated object keys, local filesystem backend, S3-compatible backend, and
  reusable retry/timeout/backoff/concurrency policy wrapper.
- Local backend reliability evidence covers stale temporary objects,
  uncommitted partial objects, missing committed objects, malformed committed
  objects, permission-denied source reads where the platform exposes them,
  immutable-write conflicts, exit-code families, and safe JSON/JSONL
  object-key/path context for the implemented local command surface. It does
  not implement repair or automatic stale temporary object cleanup.
- `fileferry-policy` has a tested parser for count-based and tag-based
  retention keep rules.
- `docs/platform-metadata.md` defines the v1 metadata capture target and
  restore reporting behavior for unrepresentable metadata. Current restore
  metadata warnings include item-level namespace, field, source platform,
  destination platform, and reason context in machine output.
- `fileferry-platform` has initial tested portable metadata capture for entry
  kind, source platform, regular-file size, timestamps where exposed by `std`,
  symlink targets, Unix mode/ownership where available, and reportable xattr
  presence/count status where the platform and filesystem expose xattr
  listing, plus ACL, file flag, resource fork, Windows attribute, and sparse
  extent status scaffolding that currently records unsupported during normal
  capture. It also has focused
  tests for path normalization facts, Windows reserved-name detection, observed case behavior, Unix symlink
  capture, xattr status capture where available, backward-compatible extension
  metadata deserialization, long-path metadata capture where the filesystem
  allows it, and permission-denied metadata reads where the platform exposes
  them.
- `fileferry-testkit` has a tested in-memory fake object store for future
  repository and pipeline tests.
- `fileferry-core` has a tested deterministic source walker with wildcard
  exclusion rules, symlink-aware metadata capture, and validated FastCDC
  content-defined chunk planning.
- `fileferry-core` has an initial tested backup pipeline that compresses
  planned chunks with zstd, encrypts chunk/index/manifest objects, writes them
  through the object-store trait, deduplicates same-content chunks by keyed
  chunk identity, and creates an encrypted snapshot manifest.
- `fileferry-core` can read back encrypted snapshot manifests and chunk indexes
  with authenticated object contexts and decrypted metadata identity checks.
- `fileferry-core` has initial tested restore primitives for manifest
  timestamps, snapshot selection by id/tag/latest, and path-scoped regular-file
  content reassembly from encrypted chunks.
- `fileferry-core` can restore directory entries, regular-file content, Unix
  symlinks, regular-file/directory modified timestamps, and regular-file and
  directory Unix permission bits where representable to a destination
  directory with destination safety checks, explicit overwrite policy, dry-run
  reporting, selected regular-file and directory creation/birth timestamp
  not-restored warnings, captured Unix ownership verification, selected
  symlink metadata not-restored warnings, selected xattr status not-restored
  warnings, selected ACL status not-restored warnings from constructed or
  future manifests, selected file flag status not-restored warnings from
  constructed or future manifests, selected resource fork status not-restored
  warnings from constructed or future manifests, selected Windows attribute
  status not-restored warnings from constructed or future manifests, selected
  sparse extent status not-restored warnings from constructed or future
  manifests, metadata planning, and optional byte-for-byte verification.
- `fileferry-core` writes commit markers after encrypted snapshot manifests,
  can discover committed manifests from those markers, and has tested snapshot
  summary and immediate-entry listing primitives for future `snapshots` and
  `ls` commands.
- `fileferry-core` writes explicit snapshot forget markers and filters
  forgotten snapshots from normal committed manifest discovery without deleting
  repository objects.
- `fileferry-web` serves the public `fileferry.app` homepage with Axum,
  server-rendered Leptos views, embedded CSS, and a `/healthz` endpoint.
- The initial product brief has been distilled into `README.md`,
  `BUILD.md`, and `AGENTS.md`.
- The product target is an all-Rust, cross-platform, encrypted backup CLI
  named `ferry`.

The repo is still pre-v1. Restore is wired into the CLI for initialized local
and S3-compatible repositories, directory entries, regular-file contents, Unix
symlinks, modified timestamps for restored regular files and directories, and
captured regular-file and directory Unix permission bits where representable.
Restore verifies captured regular-file and directory Unix ownership after
writes and warns on mismatches, but it does not change destination owners and
broader metadata application is not complete. Selected regular-file and
directory creation/birth timestamps are reported as metadata warnings because
they are not restored yet. Symlink targets are restored on Unix, but selected
symlink timestamps and captured Unix symlink mode/ownership are reported as
metadata warnings because they are not restored yet.
Reportable xattr presence/count status is captured where the platform and
filesystem expose xattr listing, but xattr names and values are reported as
metadata warnings when selected because they are not restored yet.
ACL status scaffolding is present, but normal capture currently records ACL
status as unsupported and does not read or restore ACL contents.
File flag status scaffolding is present, but normal capture currently records
file flag status as unsupported and does not read or restore file flag values.
Resource fork status scaffolding is present, but normal capture currently
records resource fork status as unsupported and does not read or restore
resource fork values.
Windows attribute status scaffolding is present, but normal capture currently
records Windows attribute status as unsupported and does not read or restore
Windows attribute values.
Sparse extent status scaffolding is present, but normal capture currently
records sparse extent status as unsupported and does not read or restore
sparse extent maps.
S3-compatible repository URLs are parsed through the shared repository
resolver, and S3-compatible `init`, `backup`, `snapshots`, `ls`, `restore`,
`check`, `forget`, `prune`, and key-management commands use the existing
encrypted object-store pipeline.
`ferry check` supports full referenced-chunk verification and deterministic
count/percentage referenced-chunk subsets for initialized local and
S3-compatible repositories. `ferry forget` is marker-only for initialized
local and S3-compatible repositories; it hides forgotten snapshots from
normal snapshot discovery but does not delete objects itself. `ferry prune`
reclaims local and S3-compatible storage through encrypted two-phase prune
plan/completion state for objects proven unreachable from non-forgotten
committed snapshots. Key
management is implemented for initialized local and S3-compatible
repositories, with `key remove` limited to marker-based removal of externally
added key slots, `key rotate` limited to unlock rotation that adds one new
slot and marker-removes explicitly selected externally added slots, and
`key export-recovery` limited to an encrypted recovery package protected by
the current repository passphrase. Recovery import, full repository rekey,
and bootstrap-slot removal are not implemented. Describe
backup, restore, check, forget, prune, key management, repository, storage,
crypto, or platform behavior only to the level backed by code, tests, and
platform evidence.

The `fileferry-web` crate is public marketing infrastructure only. It does not
turn FileFerry into a backup server, hosted product, daemon, scheduler, or web
application.

---

## Milestone G Exit Audit

Milestone G was closed on 2026-05-21 for FileFerry's current pre-v1 support
claims. The audit found no remaining concrete blocker that must be completed
before moving to format fixtures.

Exit evidence:

- Current restore code applies the implemented metadata restore subset:
  regular-file and directory modified timestamps, regular-file and directory
  Unix permission bits where representable, and Unix ownership verification
  without changing destination owners.
- Current restore code emits clear human and machine-readable metadata
  warnings for selected metadata this version does not restore, including
  creation/birth timestamps, symlink metadata, xattrs, ACL status, file flag
  status, resource fork status, Windows attribute status, and sparse extent
  status.
- `docs/platform-metadata.md`, `docs/cli-contract.md`, `README.md`, and this
  file describe those limits without claiming metadata value restoration or
  broader platform support that is not implemented and verified.
- Focused tests cover the current host-observed platform facts and restore
  warning contract: normalized relative paths, Windows reserved-name
  detection without host support claims, observed case behavior, Unix symlink
  capture/restore behavior, long-path metadata capture where the filesystem
  allows it, permission-denied metadata reads where exposed, and structured
  metadata warnings.

FileFerry still does not claim any supported platform. CI currently runs the
workspace gate on Ubuntu only, and release artifacts do not exist yet. CI,
per-target testing, smoke-tested artifacts, and release-status updates remain
Milestone I / release-hardening work. Do not reopen Milestone G for another
status-only metadata group unless it is the final blocker for a support claim
being made in the same session.

---

## Active Milestones

This section is the current execution queue. Agents should prefer completing
one active milestone end to end over making many small adjacent improvements.
Choose the first unfinished milestone that can be completed honestly in the
current session. If it is too large, split it into explicit sub-milestones in
this section before coding.

The next active milestone is Milestone H. Milestone G is closed for the current
pre-v1 support claims; support-bar work for claimed platforms belongs in
Milestone I.

### Milestone H - Format Fixtures And Compatibility Freeze

Goal: Freeze repository format `0` only after durable fixtures and migration
expectations exist.

Current status:

- Started with a narrow bootstrap/key-slot fixture slice. Golden fixture bytes
  now exist for `bootstrap`, one external `key-slots/<key-slot-id>` object,
  and one `key-slot-removals/<key-slot-id>` marker. Focused tests prove the
  current reader can open those bytes and rejects malformed, tampered, removed,
  and unsupported-version variants with the current documented error classes.
- Continued with a narrow recovery-export fixture slice. Golden fixture bytes
  now exist for one standalone encrypted recovery export package. Focused tests
  prove the current verifier can read those bytes and rejects malformed,
  unsupported-version, wrapped-master-key tamper, and master-key-check tamper
  variants with the current documented error classes.
- Continued with a committed snapshot-data fixture slice. Golden fixture bytes
  now exist for one initialized fixture repository containing one commit marker,
  one encrypted manifest, one encrypted index, and two encrypted chunks.
  Focused tests prove the current read/check/restore paths can authenticate and
  validate those bytes and reject malformed commit JSON, malformed encrypted
  framing, malformed decrypted manifest metadata, encrypted manifest/index/chunk
  tamper, wrong object name/kind context, manifest/index metadata identity
  mismatches, and unsupported commit/manifest/index schema versions.
- Continued with a forget/prune-state fixture slice. Golden fixture bytes now
  exist for one plaintext forget marker, one encrypted prune plan, one
  encrypted prune completion object, and the retained post-prune snapshot data.
  Focused tests prove current code can read, authenticate, and validate those
  bytes and reject malformed forget marker JSON, forget marker identity and
  schema mismatches, malformed prune encrypted framing and decrypted metadata,
  prune plan/completion tamper, wrong object name/kind context, prune metadata
  identity mismatches, unsupported prune schema/format versions, tampered
  completion state during recovery scanning, and stale pending prune-plan
  replay.
- Continued with a policy/config fixture slice. Golden fixture bytes now exist
  for one initialized repository with one encrypted policy/config object.
  Focused tests prove current code can read, authenticate, validate, and
  idempotently recognize those bytes and reject malformed encrypted framing,
  malformed decrypted metadata, ciphertext tamper, wrong object name/kind
  context, metadata identity mismatches, unsupported policy/config schema and
  format versions, repository identity mismatches, and invalid retention shape.
- Continued with an upload-state fixture slice. Golden fixture bytes now exist
  for one initialized repository with one encrypted upload-state object.
  Focused tests prove current code can read, authenticate, validate, and
  idempotently recognize those bytes and reject malformed encrypted framing,
  malformed decrypted metadata, ciphertext tamper, wrong object name/kind
  context, metadata identity mismatches, unsupported upload-state schema and
  format versions, repository identity mismatches, and stale upload-state
  replay when current commit/forget marker state no longer matches the marked
  state.
- Continued with a migration-detection fixture slice. Golden bootstrap bytes
  now exist for a current v0 compatibility gate, an unsupported future format
  version, unsupported v0 feature flags, and unversioned pre-v0 metadata.
  Focused tests prove current code can inspect format compatibility without
  unlocking key slots, open the current v0 fixture, reject malformed bootstrap
  JSON during inspection, reject future formats and unknown feature flags before
  unlock, and reject unversioned pre-v0 metadata instead of guessing a
  migration.
- Continued with a lease-state fixture slice. Golden fixture bytes now exist
  for one initialized repository with one encrypted `locks/<lease-id>` object.
  Focused tests prove current code can read, authenticate, validate, and
  idempotently recognize those bytes and reject malformed encrypted framing,
  malformed decrypted metadata, ciphertext tamper, wrong object name/kind
  context, metadata identity mismatches, unsupported lease-state schema and
  format versions, repository identity mismatches, invalid expiration windows,
  and expired leases for active use.
- Continued with a prune command lease-coordination slice. Non-dry-run
  `ferry prune` now uses encrypted `locks/<lease-id>` lease state before
  marking or sweeping, rejects another active readable lease as a locked
  repository, ignores expired readable leases, best-effort releases its own
  lease after the sweep path returns, and fails closed on malformed lease state
  before candidate deletion.
- Continued with a forget command lease-coordination slice. Non-dry-run
  `ferry forget` now uses encrypted `locks/<lease-id>` lease state before
  writing forget markers, rejects another active readable lease as a locked
  repository, ignores expired readable leases, best-effort releases its own
  lease after marker writes return, and fails closed on malformed lease state
  before marker writes.
- Continued with a key-management lease-coordination slice. `ferry key add`,
  `ferry key remove`, and `ferry key rotate` now use encrypted
  `locks/<lease-id>` lease state before writing key-slot objects or key-slot
  removal markers, reject another active readable lease as a locked
  repository, ignore expired readable leases, best-effort release their own
  lease after the mutation path returns, and fail closed on malformed lease
  state before key-slot mutation writes.
- Continued with a backup command lease-coordination slice. `ferry backup` now
  uses encrypted `locks/<lease-id>` lease state before publishing snapshot
  objects, rejects another active readable lease as a locked repository,
  ignores expired readable leases, best-effort releases its own lease after the
  snapshot write path returns, and fails closed on malformed lease state before
  writing chunk, index, manifest, or commit objects. Command-level lease
  enforcement is currently proven only for backup, forget, prune, and
  key-management mutation paths; repository maintenance, stale-lease breaking,
  lease repair, upload-state recovery, and broad concurrent-backup safety are
  not implemented yet.

These slices do not freeze the rest of format v0.

Definition of done:

- Add golden fixtures for bootstrap, key slots, key-slot removal markers,
  recovery exports if implemented, encrypted chunks, indexes, manifests,
  commit markers, forget markers, prune state, and lease state.
- Fixture tests prove current code can read the fixtures and rejects malformed
  or tampered variants with documented error classes.
- `docs/repository-format.md` identifies which fields are compatibility
  contracts and which remain internal.
- Migration detection and unsupported-version behavior are tested for the
  current bootstrap compatibility gate. Full migration implementation and
  cross-version compatibility remain open until a migration is intentionally
  designed.

Non-goals:

- Compatibility with restic, rustic, Borg, Kopia, rclone, or any other backup
  format.
- Freezing before recovery export and prune object formats are settled.

### Milestone I - V1 Release Hardening

Goal: Build the release evidence path after local and S3 repository behavior
meet the v1 scope.

Definition of done:

- Run documented local and S3-compatible restore drills.
- Add CI builds and tests for every platform that README or release docs call
  supported.
- Audit logs, errors, JSON, JSONL, and tests for secret leakage.
- Add release artifacts, checksums, signatures, SBOM, and
  `cargo-auditable` metadata.
- Add tested Unix shell and PowerShell install paths.
- Run release smoke tests on every claimed platform artifact.
- Update README, docs, completions, release notes, and homepage status to
  match the exact release candidate.

Non-goals:

- Tagging v1 before the exact release candidate passes.
- Claiming unsupported platforms.
- Adding GUI, TUI, daemon, server, scheduler, SaaS, mobile, FUSE, or
  compatibility-layer behavior.

## Current Deprioritized Polish

Do not choose these as primary work unless a test proves a bug or the work is
required by an active milestone:

- Reworking completed check, forget, local-backend evidence, key add, key
  remove, or key rotate slices unless a real bug blocks a remaining milestone.
- More restore edge-case diagnostics around already-covered destination
  preflight behavior.
- More check failure-envelope polish where object-key/path/snapshot context is
  already available.
- Wording-only documentation edits that do not unblock an active milestone.
- Refactors that do not remove a blocker for an active milestone.
- Broad platform, S3, metadata, prune, repair, release, or v1 claims without
  implementation and verification.
- New storage providers, repository compatibility layers, GUI/TUI/server
  surfaces, or scheduling behavior.

---

## Stack Direction

- **Rust** for every first-party binary and library.
- **Cargo workspace** with crates under `crates/`.
- **Rust 2024 edition** unless current Rust guidance or MSRV constraints argue
  otherwise before the workspace is created.
- **clap** for CLI parsing and shell completions.
- **tokio** for async storage and network work.
- **serde**, **serde_json**, and **toml** for config and machine-readable
  output.
- **tracing** for logs, spans, progress events, and redaction-aware
  diagnostics.
- **tracing-subscriber** for the CLI/logging boundary once runtime logging
  grows beyond the current command surface.
- **figment** or **config** only if profile, environment, and file layering
  outgrows the current explicit config loader.
- **miette** or **color-eyre** for human-facing diagnostics after a short
  spike confirms the best fit.
- **object_store** first for local/S3/object backends. Consider an optional
  OpenDAL adapter only after core storage semantics are proven.
- **fastcdc** for content-defined chunking.
- **blake3** for fast content IDs, checksums, and integrity trees where
  appropriate.
- **zstd** for compression.
- **Argon2id** for passphrase key derivation.
- **zeroize** and **secrecy** for secret handling.
- **xtask** only when build, fixture, compatibility, or release automation
  becomes too large for `just` and Cargo commands.
- **Axum** plus server-rendered **Leptos** for the separate public homepage
  binary.

Use current primary docs before locking crate versions, crypto primitives,
target support, or release tooling.

---

## Product Invariants

- Restore is the product.
- Every command must be scriptable.
- Stdout is data; stderr is logs, progress, and errors.
- `--json` emits one JSON document. `--jsonl` emits an event stream.
- Human output can change. Machine output is a compatibility surface.
- Client-side encryption is mandatory for repository contents and metadata.
- No plaintext file names, directory structure, indexes, snapshot metadata, or
  sensitive policy/config shape in repositories.
- The repository format is original to FileFerry.
- No restic, rustic, Borg, Kopia, or rclone repository compatibility in v1.
- Local filesystem and S3-compatible object storage are the v1 storage
  targets.
- Object storage is not a filesystem. Avoid required renames and mutable
  directory assumptions.
- Every destructive command supports `--dry-run`.
- Every long operation can be interrupted safely.
- Platform support is real only when CI, tests, and release artifacts exist.
- Cross-platform correctness beats clever single-platform fast paths.

---

## Security Model

Required v1 security properties:

- Random repository master key.
- Passphrase and key-file unlock paths for that master key.
- Envelope encryption with derived subkeys for data, metadata, and indexes.
- Authenticated encryption for every encrypted object.
- Authenticated snapshot manifests and indexes.
- Corruption and tampering detection during read, check, and restore.
- Redaction of passwords, key material, credentials, signed URLs, and secret
  environment values in logs and diagnostics.
- Secret memory handling with `zeroize`/`secrecy` where practical.
- No repository operation that silently weakens encryption for convenience.

Security design work before format freeze:

- [x] Choose AEAD and key hierarchy, then document rationale.
- [x] Define repository bootstrap plaintext and justify every field.
- [x] Define KDF parameters, migration story, and unlock UX.
- [x] Define recovery export format and user warning text.
- [x] Define key rotation semantics and what rotation does not rewrite.
- [x] Define tamper/corruption error classes and JSON output.
- [x] Add `docs/security.md`.
- [x] Add adversarial tests for wrong password, wrong key, bit flips, truncated
      objects, swapped objects, replayed indexes, and malformed metadata.

---

## Repository Model

Core objects:

- Encrypted chunks.
- Encrypted snapshot manifests.
- Encrypted indexes.
- Encrypted policy/config object.
- Temporary upload state.
- Prune marks and maintenance metadata.

Repository design goals:

- Append-friendly.
- Interruption-safe.
- Safe concurrent backups.
- No required rename operations.
- No required object listing for correctness where avoidable.
- Two-phase prune.
- Deterministic integrity checks.
- Future format migrations that can be detected and explained.

Repository format work:

- [x] Write `docs/repository-format.md` before committing to object bytes.
- [x] Define object naming without leaking source paths.
- [x] Define object authentication context and domain separation.
- [x] Define snapshot manifest structure.
- [x] Define chunk index structure.
- [x] Define commit markers and upload state.
- [x] Define repository lock or lease model, if any.
- [x] Define concurrent backup behavior.
- [x] Define prune mark, sweep, and recovery behavior.
- [ ] Add golden fixtures after the first format version is intentionally
      frozen.

---

## Target Source Layout

```text
Cargo.toml
rust-toolchain.toml
justfile
crates/
  fileferry-cli/       command parsing, output formats, config loading
  fileferry-core/      snapshots, repository format, backup/restore engine
  fileferry-storage/   local and object storage abstraction
  fileferry-crypto/    key derivation, encryption, authenticated metadata
  fileferry-platform/  filesystem metadata across Windows/macOS/Linux/BSD
  fileferry-policy/    retention, pruning, lifecycle rules
  fileferry-testkit/   fake stores, corruption tests, fixtures, helpers
  fileferry-web/       Axum + Leptos public homepage for fileferry.app
xtask/
docs/
  architecture.md
  cli-contract.md
  config.md
  homepage-deployment.md
  repository-format.md
  security.md
  storage.md
  platform-metadata.md
  operations.md
  release.md
tests/
  fixtures/
  integration/
```

Keep command presentation in `fileferry-cli`. Library crates should return typed
errors and structured events; the CLI decides human text, JSON, JSONL, and exit
codes.

---

## CLI Contract

Global flags:

```text
--repo <URL>
--profile <NAME>
--config <FILE>
--json
--jsonl
--quiet
--log-level <LEVEL>
--no-progress
```

Required v1 commands:

```text
ferry init
ferry backup
ferry restore
ferry snapshots
ferry ls
ferry check
ferry forget
ferry prune
ferry key
ferry completion
ferry version
```

High-value commands that should land before or shortly after v1 if the core is
stable:

```text
ferry find
ferry diff
ferry copy
ferry repo
ferry policy
ferry doctor
```

CLI work:

- [x] Define stable exit codes in `docs/cli-contract.md`.
- [x] Define JSON document schemas for every command.
- [x] Define JSONL event schemas for long operations.
- [x] Add golden tests for help text, JSON output, JSONL event order, and exit
      codes.
- [ ] Ensure progress bars never appear in stdout data modes.
- [ ] Ensure `--dry-run` exists for destructive commands.
- [ ] Ensure all prompts have non-interactive alternatives.

---

## Platform And Metadata

Target platforms:

- Windows x86_64 MSVC.
- Windows ARM64 MSVC.
- macOS x86_64.
- macOS ARM64.
- Linux x86_64 GNU.
- Linux x86_64 musl.
- Linux ARM64 GNU/musl.
- FreeBSD x86_64.
- NetBSD x86_64 where feasible.
- OpenBSD best-effort until CI and release support are real.

Platform work:

- [x] Define metadata capture for files, directories, symlinks, permissions,
      timestamps, ownership, xattrs, ACLs, resource forks, Windows attributes,
      and sparse extents.
- [x] Decide v1 restore behavior for metadata that cannot be represented on
      the destination platform.
- [x] Add focused tests for current host-observed path normalization facts,
      Windows reserved-name detection without host support claims, observed
      case behavior, Unix symlink behavior, long paths where the filesystem
      allows them, permission errors where exposed, and metadata warning
      output.
- [ ] Add per-target platform tests for every platform that will be claimed
      supported in a release.
- [ ] Add CI for supported platforms before claiming support.
- [ ] Add release artifacts only for platforms that pass the support bar.

---

## V1 Release Definition

FileFerry is ready for v1 only when it is boring to initialize, back up, verify,
restore, automate, and install across the supported platform list.

Minimum v1 bar:

- [x] Rust workspace exists with the target crate boundaries.
- [x] `ferry init` creates encrypted local and S3-compatible repositories.
- [x] `ferry backup` creates encrypted, compressed, deduplicated snapshots.
- [x] `ferry restore` restores by snapshot id, tag, path, and `latest`.
- [x] `ferry snapshots` and `ferry ls` have human, JSON, and JSONL-safe
      behavior where appropriate.
- [x] `ferry check` verifies metadata and configurable data subsets.
- [x] `ferry forget` and `ferry prune` implement retention and two-phase
      deletion safely.
- [x] Key add/remove/rotate/export-recovery paths exist and are tested.
- [x] Local backend passes interruption and corruption tests.
- [ ] S3-compatible backend passes retry, resume, and eventual-weirdness tests.
- [x] Stable config profiles and environment variables exist.
- [x] Shell completions are generated.
- [ ] Exit codes, JSON, and JSONL schemas are documented and tested.
- [ ] Platform metadata behavior is documented and tested on every supported
      platform.
- [ ] Release artifacts include archives, checksums, signatures, SBOM, and
      `cargo-auditable` metadata.
- [ ] Install scripts for Unix shells and PowerShell are tested.
- [x] At least one restore drill is documented from a real FileFerry snapshot.

V1 must not include GUI, TUI, FUSE mount, daemon mode, server mode, magic
scheduling, mobile apps, or compatibility with restic/rustic repositories.

---

## Phases

Ordered intent, not rigid sequence. Each phase should leave the repo in a state
where documented verification passes on a clean checkout.

### Phase 0 - Bootstrap Docs

- [x] Read local sample docs for Stephen's preferred repo documentation shape.
- [x] Write initial `README.md`, `BUILD.md`, and `AGENTS.md`.
- [x] Record the repo as pre-implementation.

### Phase 1 - Workspace Foundation

- [x] Add `rust-toolchain.toml`.
- [x] Add Cargo workspace with Rust 2024 edition.
- [x] Add crates: `fileferry-cli`, `fileferry-core`, `fileferry-storage`,
      `fileferry-crypto`, `fileferry-platform`, `fileferry-policy`, and
      `fileferry-testkit`.
- [x] Add workspace dependency policy.
- [x] Add `fileferry-cli` binary with `clap`.
- [x] Add `just fmt`, `just check`, `just test`, and `just build`.
- [x] Add GitHub Actions for formatting, clippy, tests, and build.
- [x] Add basic `ferry version`.

### Phase 2 - CLI, Config, And Output Contract

- [x] Implement config discovery and profiles.
- [x] Implement global flags and environment variable precedence.
- [x] Add typed config validation and redacted diagnostics.
- [x] Revisit `figment` or `config` only if the current explicit loader
      becomes harder to audit than a small dependency-backed layering model.
- [x] Define stable event model for command progress.
- [x] Implement human, JSON, and JSONL output surfaces.
- [x] Add CLI golden tests.
- [x] Add `ferry completion`.

### Phase 3 - Crypto And Format Design

- [x] Write `docs/security.md`.
- [x] Write `docs/repository-format.md`.
- [x] Choose AEAD, KDF parameters, and key hierarchy.
- [x] Implement master key creation and unlock.
- [x] Implement encrypted object envelope.
- [x] Add corruption and wrong-key tests.
- [ ] Freeze repository format version `0` only after fixtures exist.

### Phase 4 - Storage Backends

- [x] Implement local filesystem backend.
- [x] Implement S3-compatible backend through `object_store` or a documented
      lower-level choice.
- [x] Add storage capability model.
- [x] Add retry, timeout, concurrency, and backoff behavior.
- [x] Add fake object store in `fileferry-testkit`.
- [x] Add interruption and idempotency tests.

### Public Homepage - fileferry.app

- [x] Add a separate `fileferry-web` workspace crate so homepage dependencies
      stay out of the backup CLI/runtime crates.
- [x] Build the public homepage with Axum and server-rendered Leptos views.
- [x] Serve static CSS and a reverse-proxy-friendly `/healthz` endpoint.
- [x] Document Ubuntu self-hosting shape for `fileferry.app`.
- [x] Add route and render tests for the homepage.

### Phase 5 - Backup Pipeline

- [x] Implement source walking and exclusion rules.
- [x] Implement platform metadata capture.
- [x] Implement content-defined chunking.
- [x] Implement compression and encryption pipeline.
- [x] Implement chunk/index writes.
- [x] Implement snapshot manifest creation.
- [ ] Add resumable backup state.
- [x] Add tests for sparse trees, symlinks, permissions, large files, many
      small files, and excluded paths.

### Phase 6 - Restore Pipeline

- [x] Implement snapshot selection by id, tag, and `latest`.
- [x] Implement path-scoped restore.
- [x] Implement destination safety checks.
- [ ] Implement metadata restore per platform.
- [x] Add overwrite policy and dry-run reporting.
- [x] Add restore verification.
- [x] Add restore drill docs.

### Phase 7 - Listing, Search, And Diff

- [x] Implement `snapshots`.
- [x] Implement `ls`.
- [ ] Implement `find`.
- [ ] Implement `diff`.
- [ ] Keep output stable and machine-readable.
- [ ] Add tests for encrypted metadata lookup without leaking plaintext in the
      repository.

### Phase 8 - Check, Repair Guidance, And Doctor

- [x] Implement repository metadata check.
- [x] Implement configurable data subset checks.
- [x] Implement full read-data check.
- [x] Add deterministic corruption reports.
- [ ] Add `doctor` for environment, config, backend, and permission issues.
- [ ] Document repair guidance without promising unsafe automatic repair.

### Phase 9 - Retention And Prune

- [x] Implement retention policy parser.
- [x] Implement `forget`.
- [x] Implement local two-phase prune.
- [x] Add prune marks and recovery behavior.
- [x] Add dry-run summaries.
- [x] Add concurrent forget/prune guardrail tests.

### Phase 10 - Key Management

- [x] Implement `key add`.
- [x] Implement `key remove`.
- [x] Implement `key rotate`.
- [x] Implement `key export-recovery`.
- [ ] Document operational recovery procedures.
- [ ] Add tests for multiple unlock methods and removed keys.

### Phase 11 - Release Engineering

- [x] Add cargo-dist or documented release equivalent.
- [ ] Add Windows `.zip` artifacts.
- [ ] Add Unix `.tar.xz` artifacts.
- [ ] Add checksums and signatures.
- [ ] Add SBOM generation.
- [ ] Add `cargo-auditable` metadata.
- [ ] Add shell install script.
- [ ] Add PowerShell install script.
- [ ] Add release smoke tests.

### Phase 12 - V1 Hardening

- [ ] Run restore drills from local and S3-compatible repositories.
- [ ] Run interruption tests for backup, restore, check, forget, and prune.
- [ ] Run adversarial corruption tests.
- [ ] Run cross-platform metadata tests.
- [ ] Audit logs and diagnostics for secret leakage.
- [ ] Decide whether `tracing-subscriber` should own the final CLI logging
      boundary before v1.
- [ ] Audit JSON/JSONL stability.
- [ ] Update README, docs, completions, and release notes.
- [ ] Tag v1 only after the exact release candidate passes the evidence path.

---

## Verification

Narrowest useful command first, then broaden.

Docs-only work:

```sh
git diff --check
```

Normal Rust workspace gate once the skeleton exists:

```sh
just fmt
just check
just test
just build
```

Expected `just check` shape:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace
```

Expected checks as the project matures:

- Crypto and key-management unit tests.
- Repository format golden fixture tests.
- Corruption, tamper, and wrong-key tests.
- Fake object store idempotency and interruption tests.
- Local backend integration tests.
- S3-compatible integration tests against an isolated test bucket or emulator.
- CLI golden tests for help, JSON, JSONL, and exit codes.
- Platform metadata tests on every supported platform.
- Restore drill from real snapshots.
- Release artifact smoke tests.

If a command cannot run, report why and what was verified instead.

---

## External Sources To Re-check

Use current primary sources before implementation work that depends on
external behavior:

- Rust stable release, edition, MSRV, Cargo workspace, and target support.
- Windows MSVC, macOS, Linux GNU/musl, FreeBSD, NetBSD, and OpenBSD target
  status.
- clap, tokio, serde, tracing, miette, color-eyre, object_store, OpenDAL,
  fastcdc, blake3, zstd, secrecy, zeroize, and Argon2id crate guidance.
- Current cryptographic guidance for AEAD, KDF parameters, nonce handling, and
  key rotation.
- S3-compatible storage behavior, multipart upload behavior, retry semantics,
  consistency guarantees, and provider-specific limits.
- Azure Blob, GCS, WebDAV, and Backblaze B2 docs before adding those backends.
- cargo-dist, signing, SBOM, `cargo-auditable`, Homebrew, Scoop, WinGet,
  FreeBSD ports, and pkgsrc release docs.

Trust current primary docs and observed behavior over this file.

---

## Recent Work

- 2026-05-21 - Continued Milestone H with a backup command
  lease-coordination slice. `ferry backup` now calls the shared encrypted
  `locks/<lease-id>` mutation-lease path before snapshot publication. Active
  readable leases return the stable `repository_locked` failure code and
  repository-locked exit family before chunk, index, manifest, or commit
  writes; malformed or unauthenticatable lease state fails closed before
  snapshot writes; expired readable leases are ignored; and successful backup
  writes best-effort release their own lease. Added focused core tests for
  active-lease rejection, expired-lease tolerance plus own-lease release, and
  malformed-lease rejection before snapshot writes, plus a CLI JSON failure
  test that verifies the stable locked exit code and no snapshot-object
  publication. Updated `docs/repository-format.md`, `docs/security.md`, and
  this build plan to keep the claim narrow: this is command-level backup
  mutation coordination, not broad concurrent-backup safety, stale-lease
  repair, or upload-state recovery.
- 2026-05-21 - Continued Milestone H with a key-management
  lease-coordination slice. `ferry key add`, `ferry key remove`, and
  `ferry key rotate` now acquire encrypted `locks/<lease-id>` lease state
  before writing additional key-slot objects or key-slot removal markers.
  Active readable leases return the stable `repository_locked` failure code
  and repository-locked exit family before key-slot mutation writes; malformed
  or unauthenticatable lease state fails closed with machine-readable
  lease-object context before key-slot mutation writes. The key-management
  paths ignore expired readable leases, best-effort release their own lease
  after the mutation path returns, and focused core tests verify successful
  release. Added focused core tests for active-lease and malformed-lease
  rejection before key add/remove/rotate writes, expired-lease tolerance on the
  shared key-management lease path, plus CLI tests for stable locked/integrity
  failures and no accidental key-slot or removal-marker writes. Updated
  `docs/repository-format.md`,
  `docs/security.md`, and this build plan to document only this proven
  key-management coordination. This does not implement backup leases,
  repository-maintenance leases, stale-lease breaking, lease repair, recovery
  import, full repository rekey, or freeze all of format v0. Focused
  verification:
  `cargo test -p fileferry-core repository_key_ --all-features` and
  `cargo test -p fileferry-cli key_ --all-features`.
- 2026-05-21 - Continued Milestone H with a forget command
  lease-coordination slice. Non-dry-run `ferry forget` now acquires encrypted
  `locks/<lease-id>` lease state before writing forget markers, rejects another
  active readable lease with the stable `repository_locked` failure code and
  repository-locked exit family, ignores expired readable leases, rechecks
  locks after writing its own lease, and best-effort releases its own lease
  after marker writes return. Malformed or unauthenticatable lease state now
  fails closed before forget writes markers, with machine-readable object-key
  context for lease-state decode/validation failures. Added focused core tests
  for active-lease rejection, expired-lease tolerance plus own-lease release,
  and malformed-lease rejection before marker writes, plus CLI tests for
  stable locked/integrity failures and dry-run remaining lease-free. Updated
  `docs/repository-format.md`, `docs/security.md`, and
  `docs/cli-contract.md` to document only this proven forget/prune command
  coordination. This does not implement stale-lease breaking, lease repair,
  backup/key-management command leases, or freeze all of format v0. Focused
  verification:
  `cargo test -p fileferry-core forget_markers_with_lease -- --nocapture` and
  `cargo test -p fileferry-cli lease_has_stable -- --nocapture`.
- 2026-05-21 - Continued Milestone H with a prune command
  lease-coordination slice. Non-dry-run `ferry prune` now acquires encrypted
  `locks/<lease-id>` lease state before plan marking or sweeping, rejects
  another active readable lease with the stable `repository_locked` failure
  code and repository-locked exit family, ignores expired readable leases,
  rechecks locks after writing its own lease, and best-effort releases its own
  lease after the sweep path returns. Malformed or unauthenticatable lease
  state now fails closed before prune marks a plan or deletes candidate
  objects. Added focused core tests for active-lease rejection, expired-lease
  tolerance plus own-lease release, and malformed-lease rejection before
  deletion; updated `docs/repository-format.md`, `docs/security.md`, and
  `docs/cli-contract.md` to document only this proven prune-specific command
  coordination. This does not implement stale-lease breaking, lease repair,
  backup/forget/key-management command leases, or freeze all of format v0.
  Focused verification:
  `cargo test -p fileferry-core prune_sweep_ -- --nocapture`.
- 2026-05-21 - Continued Milestone H with the lease-state fixture slice. Added
  a dedicated `lease-state` HKDF purpose and encrypted object kind, core-only
  repository lease-state request/state/read/write APIs, active-use expiration
  validation, idempotent same-bytes writes, and CLI error-code/failure-code
  mappings for the new core error classes. Added golden fixture bytes under
  `tests/fixtures/repository-format/v0/lease-state/` for one initialized
  repository with one encrypted `locks/<lease-id>` object. Added focused
  fixture tests that unlock and read the fixture, authenticate and validate the
  lease state, idempotently recognize the same lease, reject malformed framing
  and decrypted metadata, reject ciphertext tamper, reject wrong object
  name/kind context, reject metadata identity and repository mismatches, reject
  unsupported lease-state schema and format versions, reject invalid expiration
  windows, and reject expired leases for active use. Updated
  `docs/repository-format.md` and `docs/security.md` to document only this
  proven core-only slice. That slice did not implement command-level lease
  enforcement or freeze all of format v0. Verified the focused test with
  `cargo test -p fileferry-core --test repository_format_fixtures lease_state_fixture -- --nocapture`.
- 2026-05-21 - Continued Milestone H with the migration-detection fixture
  slice. Added a narrow `fileferry-core` repository format inspection API that
  reads bootstrap compatibility metadata without unlocking key slots and
  classifies current v0, unsupported future versions, unsupported feature
  flags, and unversioned pre-v0 metadata. Added golden bootstrap fixture bytes
  under `tests/fixtures/repository-format/v0/migration/` for a future format
  version, unknown v0 feature flags, and unversioned pre-v0 metadata, while the
  current v0 path is checked against the existing bootstrap/key-slot fixture.
  Added focused fixture tests that inspect and open the current v0 fixture,
  reject malformed bootstrap JSON during inspection, reject future formats and
  unknown feature flags before unlock, and reject unversioned pre-v0 metadata
  instead of guessing a migration. Updated `docs/repository-format.md` and
  `docs/security.md` to describe only this proven compatibility-gate slice.
  This does not implement migrations or freeze all of format v0. Verified the
  focused test with
  `cargo test -p fileferry-core --test repository_format_fixtures migration_ -- --nocapture`.
- 2026-05-21 - Continued Milestone H with the upload-state fixture slice. Added
  a narrow `fileferry-core` repository upload-state object API that writes
  encrypted/authenticated upload state under `objects/upload/<writer-id>/<upload-id>`,
  records the commit and forget-marker object sets present when the state was
  marked, validates schema, magic, format version, repository id, writer/upload
  ids, object-key identity, pending object keys, and keyed state identity on
  read, and rejects stale resume attempts when current commit/forget marker
  state no longer matches the marked state. Added golden fixture bytes under
  `tests/fixtures/repository-format/v0/upload-state/` for one initialized
  repository with one encrypted upload-state object. Added focused fixture tests
  that unlock and read the fixture, idempotently recognize the same upload-state
  write, reject malformed encrypted framing and decrypted metadata, reject
  ciphertext tamper, reject wrong object names and authenticated kinds through
  AEAD context binding, reject metadata identity mismatches, reject unsupported
  upload-state schema and format versions, reject repository identity
  mismatches, and reject stale upload-state replay. Updated
  `docs/repository-format.md` and `docs/security.md` to describe only this
  proven slice. This does not freeze migration behavior or all of format v0.
  Verified the focused test with
  `cargo test -p fileferry-core --test repository_format_fixtures upload_state_fixture -- --nocapture`.
- 2026-05-21 - Continued Milestone H with the policy/config fixture slice.
  Added a narrow `fileferry-core` repository policy-config object API that
  writes encrypted/authenticated policy objects under `objects/policy/<prefix>/`
  with policy ids derived from the encrypted body, validates schema, magic,
  format version, repository id, object-key identity, retention shape, and
  metadata identity on read, and treats an already-present matching policy as an
  idempotent write result. Added golden fixture bytes under
  `tests/fixtures/repository-format/v0/policy-config/` for one initialized
  repository with one encrypted policy/config object. Added focused fixture
  tests that unlock and read the fixture, idempotently recognize the same
  policy write, reject malformed encrypted framing and decrypted metadata,
  reject ciphertext tamper, reject wrong object names and authenticated kinds
  through AEAD context binding, reject metadata identity mismatches, reject
  unsupported policy/config schema and format versions, reject repository
  identity mismatches, and reject invalid retention shape. Updated
  `docs/repository-format.md` and `docs/security.md` to describe only this
  proven slice. This does not freeze migration behavior, upload state, or all
  of format v0. Verified the focused test with
  `cargo test -p fileferry-core --test repository_format_fixtures -- --nocapture`.
- 2026-05-21 - Continued Milestone H with the forget/prune-state fixture
  slice. Added golden fixture bytes under
  `tests/fixtures/repository-format/v0/forget-prune-state/` for one initialized
  repository after a prune sweep, plus the captured plaintext forget marker
  deleted by that sweep. The fixture covers one forget marker, one encrypted
  prune plan, one encrypted prune completion object, and retained post-prune
  snapshot data. Added focused `fileferry-core` fixture tests that unlock the
  fixture, read forgotten snapshot ids, authenticate and validate prune plan and
  completion state, check the retained snapshot data, reject malformed forget
  marker JSON, reject forget marker schema and identity mismatches, reject
  malformed prune encrypted-object framing and decrypted metadata, reject prune
  plan/completion ciphertext tampering, reject wrong object names and
  authenticated kinds through AEAD context binding, reject prune metadata
  identity mismatches, reject unsupported prune schemas and format versions,
  reject tampered completion state during prune recovery scanning, and reject a
  stale pending prune-plan replay when current commit/forget marker state no
  longer matches the marked plan. Tightened prune recovery scanning so a
  completion object must decrypt and validate before a marked plan is treated as
  complete, added prune completion error-code mapping, and updated
  `docs/repository-format.md`, `docs/security.md`, and
  `docs/cli-contract.md` to describe only this proven slice. This does not
  freeze migration behavior, policy/config objects, upload state, or all of
  format v0. Verified the focused test with
  `cargo test -p fileferry-core --test repository_format_fixtures -- --nocapture`.
- 2026-05-21 - Continued Milestone H with the committed snapshot-data fixture
  slice. Added golden fixture bytes under
  `tests/fixtures/repository-format/v0/snapshot-data/` for one initialized
  repository with one plaintext commit marker, one encrypted snapshot manifest,
  one encrypted chunk index, and two encrypted chunks. Added focused
  `fileferry-core` fixture tests that unlock the fixture, read committed
  manifests, read the chunk index, run full repository check, restore selected
  file bytes, reject malformed commit JSON, reject malformed encrypted-object
  framing and decrypted manifest metadata, reject manifest/index/chunk
  ciphertext tampering, reject wrong object names and object kinds through AEAD
  context binding, reject manifest/index metadata identity mismatches, and
  reject unsupported commit, manifest, and index schema versions. Tightened
  current manifest/index readers so unsupported decrypted schema versions fail
  closed, added the `chunk_index_invalid` CLI failure code mapping, and updated
  `docs/repository-format.md`, `docs/security.md`, and `docs/cli-contract.md`
  to describe only this proven slice. This does not freeze forget markers,
  prune state, policy/config objects, upload state, migrations, or all of
  format v0. Verified the focused test with
  `cargo test -p fileferry-core --test repository_format_fixtures -- --nocapture`.
- 2026-05-21 - Continued Milestone H with the recovery-export fixture slice.
  Added golden fixture bytes under
  `tests/fixtures/repository-format/v0/recovery-export/` for one standalone
  encrypted recovery export package. Added a small `fileferry-core` recovery
  export verifier and focused fixture tests that verify the fixture with the
  primary passphrase, reject malformed recovery-export JSON, reject unsupported
  recovery-export format versions, reject tampered wrapped-master-key
  ciphertext, and reject a tampered recovery-export `master_key_check`. Updated
  `docs/repository-format.md` and `docs/security.md` to identify exactly what
  this slice proves. This does not implement recovery import and does not
  freeze chunks, indexes, manifests, commit markers, forget markers, prune
  state, migrations, or all of format v0. Verified the focused test with
  `cargo test -p fileferry-core --test repository_format_fixtures -- --nocapture`.
- 2026-05-21 - Started Milestone H with the first repository-format fixture
  slice. Added golden fixture bytes under
  `tests/fixtures/repository-format/v0/bootstrap-keyslot/` for the plaintext
  bootstrap object, an externally added passphrase key slot, and a marker-based
  key-slot removal. Added focused `fileferry-core` fixture tests that open the
  fixture with the primary and added passphrases, verify the removal marker
  hides the retired slot, reject malformed bootstrap JSON, reject unsupported
  bootstrap format versions, reject a tampered external key-slot
  `master_key_check`, and reject a tampered removal-marker
  `master_key_removal_check`. Updated `docs/repository-format.md` to identify
  the compatibility-facing fields for this slice and to keep the broader
  format-v0 freeze limits explicit. This does not freeze chunks, indexes,
  manifests, commit markers, forget markers, prune state, recovery exports, or
  all of format v0. Verified the focused test with
  `cargo test -p fileferry-core --test repository_format_fixtures -- --nocapture`.
- 2026-05-21 - Completed the Milestone G exit audit and closed Milestone G
  without adding another status-only metadata group. The audit compared the
  Milestone G definition of done against `docs/platform-metadata.md`, current
  restore implementation, focused platform tests, CLI/core metadata warning
  tests, README support language, release docs, and CI configuration. The
  remaining broad items are support-bar work: per-target CI, per-target
  platform tests, release artifacts, and release smoke tests. Because
  FileFerry does not currently claim any supported platform and has no release
  artifacts, that work belongs in Milestone I / release hardening, not in the
  active metadata-hardening queue. Milestone H - Format Fixtures And
  Compatibility Freeze is now the next active milestone. No Rust code changed
  and no live S3 gates were enabled in this session. Verified with
  `git diff --check`.
- 2026-05-21 - Completed a Milestone G sparse extent status scaffolding and
  restore warning slice without claiming sparse extent value capture, sparse
  extent restoration, or broader platform support. New manifests now include a
  sparse extent status field under encrypted platform extensions; normal
  capture records it as unsupported until platform-specific sparse extent
  capture is implemented and verified. Restore planning carries selected
  sparse extent status from constructed or future manifests, counts selected
  captured/denied sparse extent status in `metadata_planned`, and emits
  structured source-platform namespace / `sparse_extents` metadata warnings
  when sparse extents were observed or sparse extent capture was denied
  because this version does not restore sparse extent maps. Older extension
  metadata that only contains xattr, ACL, file flag, resource fork, or Windows
  attribute status still deserializes with sparse extent status defaulted to
  unsupported. Metadata capture summaries now treat denied sparse extent status
  as partial metadata capture. Updated `README.md`,
  `docs/cli-contract.md`, `docs/platform-metadata.md`, and this file. No live
  S3 gates were enabled in this session. Verified with
  `cargo test -p fileferry-platform sparse_extent -- --nocapture`,
  `cargo test -p fileferry-platform deserializes_xattr_only_extensions_with_default_extension_statuses -- --nocapture`,
  `cargo test -p fileferry-core plan_unrestored_platform_extensions_warns_for_sparse_extent_status -- --nocapture`,
  `cargo test -p fileferry-core metadata_status_counts_sparse_extent_denial_as_partial -- --nocapture`,
  and `cargo test -p fileferry-platform -p fileferry-core -p fileferry-cli
  --no-fail-fast`, `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-21 - Completed a Milestone G resource fork status scaffolding and
  restore warning slice without claiming resource fork value capture, resource
  fork restoration, or broader macOS platform support. New manifests now
  include a resource fork status field under encrypted platform extensions;
  normal capture records it as unsupported until platform-specific resource
  fork capture is implemented and verified. Restore planning carries selected
  resource fork status from constructed or future manifests, counts selected
  captured/denied resource fork status in `metadata_planned`, and emits
  structured `macos` namespace / `resource_forks` metadata warnings when
  resource forks were observed or resource fork capture was denied because
  this version does not restore resource fork values. Older extension metadata
  that only contains xattr, ACL, file flag, or Windows attribute status still
  deserializes with resource fork status defaulted to unsupported. Metadata
  capture summaries now treat denied resource fork status as partial metadata
  capture. Updated `README.md`, `docs/cli-contract.md`,
  `docs/platform-metadata.md`, and this file. No live S3 gates were enabled
  in this session. Verified with
  `cargo test -p fileferry-platform resource_fork -- --nocapture`,
  `cargo test -p fileferry-platform deserializes_xattr_only_extensions_with_default_extension_statuses -- --nocapture`,
  `cargo test -p fileferry-core plan_unrestored_platform_extensions_warns_for_resource_fork_status -- --nocapture`,
  `cargo test -p fileferry-core metadata_status_counts_resource_fork_denial_as_partial -- --nocapture`,
  `cargo test -p fileferry-platform -p fileferry-core -p fileferry-cli --no-fail-fast`,
  `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-21 - Completed a Milestone G Windows attribute status scaffolding
  and restore warning slice without claiming Windows attribute value capture,
  Windows attribute restoration, or broader Windows platform support. New
  manifests now include a Windows attribute status field under encrypted
  platform extensions; normal capture records it as unsupported until
  platform-specific Windows attribute capture is implemented and verified.
  Restore planning carries selected Windows attribute status from constructed
  or future manifests, counts selected captured/denied Windows attribute
  status in `metadata_planned`, and emits structured `windows` namespace /
  `windows_attributes` metadata warnings when Windows attributes were observed
  or Windows attribute capture was denied because this version does not
  restore Windows attribute values. Older extension metadata that only
  contains xattr, ACL, and file flag status still deserializes with Windows
  attribute status defaulted to unsupported. Updated `README.md`,
  `docs/cli-contract.md`, `docs/platform-metadata.md`, and this file. No live
  S3 gates were enabled in this session. Verified with
  `cargo test -p fileferry-platform windows_attribute -- --nocapture`,
  `cargo test -p fileferry-platform deserializes_xattr_only_extensions_with_default_extension_statuses -- --nocapture`,
  `cargo test -p fileferry-core plan_unrestored_platform_extensions_warns_for_windows_attribute_status -- --nocapture`,
  `cargo test -p fileferry-platform -p fileferry-core -p fileferry-cli --no-fail-fast`,
  `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-21 - Completed a Milestone G file flag status scaffolding and
  restore warning slice without claiming file flag value capture, file flag
  restoration, or broader platform support. New manifests now include a file
  flag status field under encrypted platform extensions; normal capture
  records it as unsupported until platform-specific file flag capture is
  implemented and verified. Restore planning carries selected file flag status
  from constructed or future manifests, counts selected captured/denied file
  flag status in `metadata_planned`, and emits structured source-platform
  namespace / `file_flags` metadata warnings when file flags were observed or
  file flag capture was denied because this version does not restore file flag
  values. Older extension metadata that only contains xattr and ACL status
  still deserializes with file flag status defaulted to unsupported. Updated
  `README.md`, `docs/cli-contract.md`, `docs/platform-metadata.md`, and this
  file. No live S3 gates were enabled in this session. Verified with `cargo
  test -p fileferry-platform file_flag -- --nocapture`, `cargo test -p
  fileferry-platform deserializes_xattr_only_extensions_with_default_acl_status
  -- --nocapture`, `cargo test -p fileferry-core
  plan_unrestored_platform_extensions_warns_for_file_flag_status --
  --nocapture`, `cargo test -p fileferry-platform -p fileferry-core -p
  fileferry-cli --no-fail-fast`, `just fmt`, `just check`, `just test`, `just
  build`, and `git diff --check`.
- 2026-05-21 - Completed a Milestone G ACL status scaffolding and restore
  warning slice without claiming ACL value capture, ACL restoration, or broader
  platform support. New manifests now include an ACL status field under
  encrypted platform extensions; normal capture records it as unsupported until
  platform-specific ACL capture is implemented and verified. Restore planning
  carries selected ACL status from constructed or future manifests, counts
  selected captured/denied ACL status in `metadata_planned`, and emits
  structured source-platform namespace / `acls` metadata warnings when ACLs
  were observed or ACL capture was denied because this version does not restore
  ACL contents. Older extension metadata that only contains xattr status still
  deserializes with ACL status defaulted to unsupported. Updated `README.md`,
  `docs/cli-contract.md`, `docs/platform-metadata.md`, and this file. No live
  S3 gates were enabled in this session. Verified with `cargo test -p
  fileferry-platform acl -- --nocapture`, `cargo test -p fileferry-core
  plan_unrestored_platform_extensions_warns_for_acl_status -- --nocapture`,
  `cargo test -p fileferry-platform -p fileferry-core -p fileferry-cli
  --no-fail-fast`, `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-21 - Completed a Milestone G reportable xattr status warning slice
  without claiming broader platform support or full xattr restoration. New
  manifests now record reportable xattr presence/count status where the
  platform and filesystem expose xattr listing. Restore carries selected xattr
  status through core destination planning, counts selected reportable xattr
  fields in `metadata_planned`, and emits structured source-platform
  namespace / `xattrs` metadata warnings when reportable xattrs were observed
  or xattr capture was denied because this version does not restore xattr
  names or values. The observed macOS `com.apple.provenance` implementation
  detail is filtered out of reportable xattr counts so normal temp-file
  restores do not become partial successes from provenance alone. Updated
  `README.md`, `docs/cli-contract.md`, `docs/platform-metadata.md`, and this
  file. No live S3 gates were enabled in this session. Verified with
  `cargo test -p fileferry-platform xattr -- --nocapture`, `cargo test -p
  fileferry-core restore_snapshot_to_destination_warns_for_unrestored_xattrs_when_present
  -- --nocapture`, `cargo test -p fileferry-cli restore_ --test
  repository_commands -- --nocapture`, `cargo test -p fileferry-platform -p
  fileferry-core -p fileferry-cli --no-fail-fast`, `just fmt`, `just check`,
  `just test`, `just build`, and `git diff --check`.
- 2026-05-21 - Completed a Milestone G regular-file and directory
  creation/birth timestamp warning slice without claiming broader platform
  support. Restore now carries selected regular-file and directory
  creation/birth timestamps through core destination planning, counts those
  selected fields in `metadata_planned`, continues to restore content and
  modified timestamps as before, and emits structured `portable`/`created`
  metadata warnings because this version does not restore creation/birth time.
  JSON, JSONL, and human restore output now return partial-success exit code
  `10` when those metadata warnings are the only issue. Updated `README.md`,
  `docs/cli-contract.md`, `docs/platform-metadata.md`, and this file. No live
  S3 gates were enabled in this session. Verified with `cargo test -p
  fileferry-core created_timestamp --no-fail-fast`, `cargo test -p
  fileferry-core restore_snapshot_to_destination --no-fail-fast`, `cargo test
  -p fileferry-cli restore_ --test repository_commands --no-fail-fast`,
  `cargo fmt --all`, `cargo test -p fileferry-platform -p fileferry-core -p
  fileferry-cli --no-fail-fast`, `just fmt`, `just check`, `just test`,
  `just build`, and `git diff --check`.
- 2026-05-21 - Completed a Milestone G symlink metadata warning slice without
  claiming broader platform support. Restore now carries selected symlink
  modified timestamp, creation timestamp, Unix mode, and Unix UID/GID metadata
  through core destination planning, counts those selected symlink fields in
  `metadata_planned`, restores the symlink target as before, and emits
  structured `portable`/`modified`, `portable`/`created`, `unix`/`mode`,
  `unix`/`uid`, and `unix`/`gid` metadata warnings because this version does
  not restore symlink metadata. JSON restore of selected symlink entries now
  returns partial-success exit code `10` when those metadata warnings are the
  only issue. Updated `README.md`, `docs/cli-contract.md`,
  `docs/platform-metadata.md`, and this file. No live S3 gates were enabled in
  this session. Verified with `cargo test -p fileferry-core symlink
  --no-fail-fast`, `cargo test -p fileferry-cli symlink --test
  repository_commands --no-fail-fast`, `cargo test -p fileferry-platform -p
  fileferry-core -p fileferry-cli --no-fail-fast`, `just fmt`, `just check`,
  `just test`, and `just build`.
- 2026-05-21 - Completed a Milestone G Unix ownership restore-warning slice
  without claiming broader platform support. Restore now carries captured Unix
  UID/GID metadata for regular files and directories through core restore
  planning, counts captured ownership fields in `metadata_planned`, verifies
  after writes whether destination UID/GID already matches the captured
  ownership, and emits structured `unix`/`uid` or `unix`/`gid` metadata
  warnings when ownership is unrepresentable, cannot be observed, or differs
  from the captured value. Restore does not call `chown` or change destination
  owners. CLI restore metadata field count expectations now include captured
  Unix ownership fields on Unix. Updated `README.md`,
  `docs/cli-contract.md`, `docs/platform-metadata.md`, and this file. No live
  S3 gates were enabled in this session. Verified with `cargo test -p
  fileferry-core unix_owner --no-fail-fast`, `cargo test -p fileferry-core
  restore_snapshot_to_destination --no-fail-fast`, `cargo test -p
  fileferry-cli restore_ --no-fail-fast`, `cargo test -p fileferry-platform
  -p fileferry-core -p fileferry-cli --no-fail-fast`, `just fmt`, `just
  check`, `just test`, `just build`, and `git diff --check`.
- 2026-05-21 - Completed a Milestone G restore destination guardrail slice
  without claiming broader platform support. Non-dry-run restore now preflights
  selected manifest paths for collisions when the destination filesystem is
  observed as case-insensitive, rejects symlinked destination ancestors while
  probing the nearest existing destination directory, and rejects Windows
  reserved-name path segments on Windows destinations before destination
  writes. The platform case-behavior probe now uses a unique temporary probe
  filename instead of a fixed spelling. CLI JSON/JSONL failure envelopes map
  the new guardrails to stable `restore_destination_path_collision` and
  `restore_destination_reserved_name` codes with path context. Updated
  `README.md`, `docs/cli-contract.md`, `docs/platform-metadata.md`,
  `docs/operations.md`, and this file. No live S3 gates were enabled in this
  session. Verified with `cargo test -p fileferry-core
  restore_destination_guardrails --no-fail-fast`, `cargo test -p
  fileferry-core case_collisions --no-fail-fast`, `cargo test -p
  fileferry-cli restore_destination_guardrail_failures_have_stable_machine_codes
  --no-fail-fast`, `cargo test -p fileferry-platform
  probes_observed_case_behavior_for_temp_directory --no-fail-fast`, `cargo
  test -p fileferry-platform -p fileferry-core -p fileferry-cli
  --no-fail-fast`, `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-20 - Completed a Milestone G Unix permission restore slice without
  claiming broader platform support. Restore now carries captured Unix mode
  metadata for regular files and directories through core restore planning,
  applies ordinary permission bits (`0o777`) on Unix destinations after entry
  writes, counts captured Unix mode fields in `metadata_planned`, and records
  structured `unix`/`mode` warnings for unrepresentable destination platforms,
  apply failures, or captured special mode bits that are intentionally not
  restored. CLI restore tests now expect the additional Unix metadata fields
  in JSON/JSONL output on Unix and verify restored file permission bits
  through the CLI. Updated `README.md`, `docs/cli-contract.md`,
  `docs/platform-metadata.md`, and this file. No live S3 gates were enabled in
  this session. Verified with `cargo test -p fileferry-core
  restore_snapshot_to_destination --no-fail-fast`, `cargo test -p
  fileferry-cli restore_ --no-fail-fast`, `just fmt`, `cargo test -p
  fileferry-platform -p fileferry-core -p fileferry-cli --no-fail-fast`,
  `just check`, `just test`, `just build`, and `git diff --check`.
- 2026-05-20 - Completed a Milestone G metadata warning contract hardening
  slice without claiming broader platform support. Captured entry metadata now
  records the source platform for new manifests and deserializes older v0
  manifests without that field as `unknown`. Restore metadata warnings now
  carry snapshot entry id, metadata namespace, metadata field, source
  platform, destination platform, and reason through core results and CLI JSON
  and JSONL output while preserving human stderr warnings and partial-success
  exit code `10`. Added platform-owned tests for normalized relative path
  facts, Windows reserved-name detection without host support claims, observed
  case behavior, Unix symlink target capture, long-path metadata capture where
  the filesystem allows it, permission-denied metadata reads where exposed,
  and backwards-compatible metadata deserialization. Updated
  `docs/cli-contract.md`, `docs/platform-metadata.md`,
  `docs/repository-format.md`, and this file. Verified with
  `cargo test -p fileferry-platform --no-fail-fast`, `cargo test -p
  fileferry-core restore_snapshot_to_destination --no-fail-fast`, `cargo test
  -p fileferry-cli restore_jsonl_metadata_warnings_stay_on_stdout
  --no-fail-fast`, `cargo test -p fileferry-core -p fileferry-cli
  --no-fail-fast`, `just fmt`, `just check`, `just test`, and `just build`.
- 2026-05-20 - Completed Milestone F S3 two-phase prune implementation
  without claiming new live provider evidence. `ferry prune` now accepts
  initialized S3-compatible repositories through the shared S3 repository
  resolver and the same encrypted object-store prune pipeline as local
  repositories. S3 prune writes immutable encrypted prune-plan and
  prune-completion state, does not require rename operations for correctness,
  supports dry-run/sweep/resume behavior through the existing core path, and
  keeps repair of arbitrary S3 corruption and provider lifecycle policies out
  of scope. Added CLI coverage proving S3 prune requires explicit S3
  environment before password access, added a gated
  `FILEFERRY_S3_PRUNE_INTEGRATION=1` live drill for init, backup, forget,
  prune dry-run, prune sweep, snapshots, durable prune state, and
  unique-prefix cleanup, added core coverage for stale/unknown retained
  listed objects and delete permission failure, and added storage policy
  coverage for retryable delete failures. The live S3 prune gate was not
  enabled in this session, so no new Backblaze provider contact or provider
  evidence was produced. Updated `README.md`, `docs/cli-contract.md`,
  `docs/repository-format.md`, `docs/storage.md`, `docs/operations.md`,
  `docs/backblaze-b2-dev-storage.md`, and this file. Verified with
  `cargo test -p fileferry-cli s3_data_path_commands_require_s3_environment_before_password --no-fail-fast`,
  `cargo test -p fileferry-cli s3_prune_live_integration_when_env_is_enabled --no-fail-fast`
  (gated; env unset, so no live provider contact), `cargo test -p
  fileferry-core prune_ --no-fail-fast`, `cargo test -p fileferry-storage
  policy_store_retries_retryable_delete_errors --no-fail-fast`, `cargo test
  -p fileferry-core -p fileferry-cli --no-fail-fast`, `just fmt`,
  `just check`, `just test`, `just build`, and `git diff --check`.
- 2026-05-20 - Completed Milestone E S3 retention and key-management command
  parity without claiming new live provider evidence. S3-compatible `forget`,
  `key add`, `key remove`, `key rotate`, and `key export-recovery` now use
  the shared S3 repository resolver and the same encrypted core object-store
  pipeline as local repositories; S3-compatible `prune` remains explicitly
  unsupported with exit code `9` before password or S3 credential access.
  Added CLI coverage proving S3 retention/key commands require explicit S3
  environment before password access, kept the unsupported S3 prune guardrail,
  and added a gated
  `FILEFERRY_S3_RETENTION_KEY_INTEGRATION=1` live drill for init, backup,
  forget, snapshots, key add, key remove, key rotate, key export-recovery,
  removed-key unlock failure, and unique-prefix cleanup. The live gate was not
  enabled in this session, so no new Backblaze provider contact or provider
  evidence was produced. Updated `README.md`, `docs/cli-contract.md`,
  `docs/storage.md`, `docs/operations.md`,
  `docs/backblaze-b2-dev-storage.md`, and this file. Verified with
  `cargo test -p fileferry-cli s3_data_path_commands_require_s3_environment_before_password --no-fail-fast`,
  `cargo test -p fileferry-cli unsupported_s3_repository_commands_fail_before_credentials_are_required --no-fail-fast`,
  `cargo test -p fileferry-cli s3_retention_key_management_live_integration_when_env_is_enabled --no-fail-fast`
  (gated; env unset, so no live provider contact), `cargo test -p
  fileferry-cli key_ --no-fail-fast`, `cargo test -p fileferry-cli forget_
  --no-fail-fast`, `cargo test -p fileferry-core repository_key
  --no-fail-fast`, `cargo test -p fileferry-core forget_ --no-fail-fast`,
  `cargo test -p fileferry-core -p fileferry-cli --no-fail-fast`,
  `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-20 - Completed Milestone D2 S3 data-path provider evidence against
  the private Backblaze B2 development bucket `dunamismax-b2` in region
  `us-east-005`, using unique prefixes under `fileferry/dev`. Hardened the
  gated CLI live test so S3 command failures are reported with redacted
  stdout/stderr instead of formatting the command environment, then ran the
  live drill with `FILEFERRY_S3_DATA_INTEGRATION=1`. The drill created an
  initialized S3-compatible repository, ran `init`, `backup`, `snapshots`,
  `ls`, `restore`, and `check`, verified restored bytes, verified successful
  check readback of referenced encrypted chunks, deleted a referenced
  manifest object, confirmed `ferry check --json` failed closed with exit code
  `6`, `repository_check_missing_object`, and machine-readable `object_key`
  and `finding.object_key` context, and cleaned up only the unique test
  prefix. The storage live round trip also passed against the same Backblaze
  environment with conditional create disabled for B2. S3-compatible
  retention, prune, and key management remain unimplemented. Verified with
  `FILEFERRY_S3_DATA_INTEGRATION=1 cargo test -p fileferry-cli
  s3_data_path_live_integration_when_env_is_enabled --no-fail-fast` and
  `FILEFERRY_S3_INTEGRATION=1 cargo test -p fileferry-storage
  s3_store_round_trip_when_integration_env_is_enabled --no-fail-fast`.
  Additional non-live verification passed with `cargo test -p fileferry-core
  -p fileferry-cli --no-fail-fast`, `just fmt`, `just check`, `just test`,
  `just build`, and `git diff --check`.
- 2026-05-20 - Implemented the Milestone D S3 backup/read command slice
  without claiming live provider verification. `ferry backup`, `ferry
  snapshots`, `ferry ls`, `ferry restore`, and `ferry check` now accept
  initialized S3-compatible repositories through the shared repository store
  resolver, using the same encrypted core pipeline and CLI JSON/JSONL output
  shapes as local repositories. S3-compatible `forget`, `prune`, and key
  management remain unsupported before password or S3 credential access with
  exit code `9` and `repository_backend_unsupported`. Added CLI coverage for
  S3 data-path commands requiring explicit S3 environment before password
  access, kept unsupported S3 coverage for retention/key/prune paths, and
  added a gated `FILEFERRY_S3_DATA_INTEGRATION=1` live data-path drill that
  runs init, backup, snapshots, ls, restore, check, and a missing referenced
  manifest failure under an isolated prefix. S3 live environment variables
  were unset in this session, so Milestone D2 remains active for provider
  evidence before S3 retention or key-management work. Updated `README.md`,
  `docs/cli-contract.md`, `docs/storage.md`, `docs/operations.md`, and this
  file. Verified with `cargo test -p fileferry-cli
  s3_data_path_commands_require_s3_environment_before_password
  --no-fail-fast`, `cargo test -p fileferry-cli
  unsupported_s3_repository_commands_fail_before_credentials_are_required
  --no-fail-fast`, `cargo test -p fileferry-cli
  s3_data_path_live_integration_when_env_is_enabled --no-fail-fast` (gated;
  env unset, so no live provider contact), `cargo test -p fileferry-cli
  s3_repository --no-fail-fast`, `cargo test -p fileferry-cli
  repository_open_failures_are_structured_and_redacted_in_machine_modes
  --no-fail-fast`, `cargo test -p fileferry-core -p fileferry-cli
  --no-fail-fast`, `cargo fmt --all`, `just fmt`, `just check`, `just test`,
  `just build`, and `git diff --check`.
- 2026-05-19 - Completed Milestone C S3 command parity foundation without
  claiming S3 data-path support. Replaced the CLI's non-init local-only
  repository resolver with a shared local/S3 repository target and store
  resolver, kept S3-compatible `init` on the existing explicit environment
  contract, and added command-level backend support gates. S3-compatible
  `backup`, `restore`, `snapshots`, `ls`, `check`, `forget`, `prune`, and key
  management paths now fail before password or S3 credential access with
  exit code `9`, `repository_backend_unsupported`, and redacted
  `s3://<redacted>` repository URLs. Invalid S3 URLs with embedded
  credentials, query strings, or fragments still fail during URL parsing
  before use. Updated `README.md`, `docs/cli-contract.md`,
  `docs/storage.md`, and this file; removed Milestone C from the Active
  Milestones queue so Milestone D is next. Verified with
  `cargo test -p fileferry-cli repository_open_failures_are_structured_and_redacted_in_machine_modes --no-fail-fast`,
  `cargo test -p fileferry-cli s3_repository_commands_fail_as_unsupported_before_credentials_are_required --no-fail-fast`,
  `cargo test -p fileferry-cli s3_repository --no-fail-fast`,
  `cargo test -p fileferry-cli init_s3 --no-fail-fast`,
  `cargo test -p fileferry-core -p fileferry-cli --no-fail-fast`,
  `cargo fmt --all`, `just fmt`, `just check`, `just test`, `just build`,
  and `git diff --check`.
- 2026-05-19 - Completed Milestone B local two-phase prune for initialized
  local repositories by implementing `ferry prune`. The command unlocks with
  `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, supports `--dry-run`,
  computes objects reachable from forgotten committed snapshots and not
  reachable from non-forgotten committed snapshots, writes encrypted prune
  plan state before sweeping, deletes only planned commit markers, forget
  markers, manifests, indexes, and chunks, and writes encrypted completion
  state after the sweep. Real prune resumes an incomplete plan when the
  repository commit/forget marker state still matches that plan, treats
  missing candidate objects as already gone, and aborts with
  `repository_prune_state_changed` if a new commit or forget marker appears.
  Prune never deletes bootstrap, key slots, key-slot removal markers, policy
  objects, upload state, prune state, unknown objects, or non-forgotten
  snapshot data. S3-compatible prune, repair, stale temporary cleanup, and
  repository compaction beyond unreachable-object deletion remain
  unimplemented. Added core and CLI tests for dry-run, successful sweep,
  interruption/resume, missing candidates, malformed prune state,
  commit/forget state-change guardrails, JSON/JSONL output, and exit-code
  mapping. Updated `README.md`, `docs/repository-format.md`,
  `docs/cli-contract.md`, `docs/operations.md`, and this file. Verified with
  `cargo test -p fileferry-core prune_ --no-fail-fast`,
  `cargo test -p fileferry-cli prune_ --no-fail-fast`,
  `cargo test -p fileferry-core -p fileferry-cli --no-fail-fast`,
  `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-19 - Completed Milestone A key recovery export for initialized
  local repositories by implementing
  `ferry key export-recovery --output <FILE>`. The command unlocks with
  `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, writes a standalone
  encrypted recovery package to a destination file that must not already
  exist, protects that package with the current repository passphrase, and
  reports human, JSON, and JSONL-safe output without printing raw master-key
  material, passphrases, or unredacted destinations. The recovery package
  records repository id, export id, creation time, warning text, Argon2id v1.3
  KDF parameters and salt, XChaCha20-Poly1305 nonce and encrypted master-key
  bytes, and a keyed master-key check for future verification. Recovery
  import, plaintext key export, full repository rekey, bootstrap-slot rewrite,
  repository-object re-encryption, lost-passphrase recovery, and S3-compatible
  key management remain unimplemented. Added crypto, core, and CLI tests for
  success, authenticated context binding, tamper failure, wrong password,
  destination safety, malformed-output prevention, redaction, JSON/JSONL
  output, and exit-code mapping. Updated `README.md`,
  `docs/security.md`, `docs/repository-format.md`, `docs/cli-contract.md`,
  and this file. Verified with
  `cargo test -p fileferry-crypto recovery_key_export --no-fail-fast`,
  `cargo test -p fileferry-core repository_recovery_export --no-fail-fast`,
  `cargo test -p fileferry-cli key_export_recovery --no-fail-fast`,
  `cargo test -p fileferry-core -p fileferry-cli --no-fail-fast`,
  `just fmt`, `just check`, `just test`, and `just build`.
- 2026-05-19 - Cleaned up `BUILD.md` after completion of the previous active
  milestones. Removed completed milestone definitions from the Active
  Milestones queue, replaced them with a forward-looking v1 execution queue
  covering recovery export, local prune, S3 command parity, S3 prune,
  metadata/platform hardening, format fixtures, and release hardening, and
  tightened deprioritized-work guidance so future agents focus on remaining
  v1 blockers instead of revisiting completed slices. Also clarified that the
  canonical GitHub remote is HTTPS while local checkouts may use SSH aliases
  and multiple push URLs. Verified with `git diff --check`.
- 2026-05-19 - Completed Milestone 7 key rotate unlock slice for initialized
  local repositories by implementing
  `ferry key rotate --retire-key-slot <KEY_SLOT_ID>...`. The command unlocks
  with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, writes one
  immutable new `key-slots/<key-slot-id>` object from `--new-password-file`,
  `FILEFERRY_NEW_PASSWORD`, or `FILEFERRY_NEW_PASSWORD_FILE`, proves the new
  passphrase unlock path wraps the existing repository master key, and writes
  immutable `key-slot-removals/<key-slot-id>` markers for explicitly selected
  externally added key slots. Rotated-away selected slots no longer unlock,
  while the new passphrase and any unselected remaining unlock paths still do.
  The command does not create a new master key, delete key-slot objects,
  remove the original bootstrap slot, remove unselected slots, rewrite or
  re-encrypt repository objects, recover lost keys, or implement
  S3-compatible key management. Added human/JSON/JSONL-safe CLI output,
  structured failure mapping for wrong passwords, missing selected slots,
  malformed key-slot objects, malformed removal markers, and redaction, plus
  security, repository-format, CLI contract, README, and build-plan docs.
  Verified with `cargo test -p fileferry-core repository_key_rotate
  --no-fail-fast`, `cargo test -p fileferry-cli key_rotate --no-fail-fast`,
  `cargo test -p fileferry-core -p fileferry-cli --no-fail-fast`,
  `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-19 - Completed Milestone 6 key remove first slice for initialized
  local repositories by implementing `ferry key remove <KEY_SLOT_ID>` for
  externally added key slots. The command unlocks with `FILEFERRY_PASSWORD` or
  `FILEFERRY_PASSWORD_FILE`, proves the supplied passphrase still unlocks a
  remaining non-removed slot before writing removal state, and writes one
  immutable `key-slot-removals/<key-slot-id>` marker. Removed slots no longer
  unlock after the marker verifies, while the proven remaining passphrase
  still unlocks. The command does not delete `key-slots/<key-slot-id>`
  objects, remove the original bootstrap slot, create a new master key,
  rewrite or re-encrypt repository objects, recover lost keys, or implement
  S3-compatible key management. Added human/JSON/JSONL-safe CLI output,
  idempotent marker handling, structured failure mapping for wrong passwords,
  missing slot ids, malformed removal markers, and lockout prevention, plus
  security, repository-format, CLI contract, README, and build-plan docs.
  Verified with `cargo test -p fileferry-core repository_key_remove
  --no-fail-fast`, `cargo test -p fileferry-cli key_remove --no-fail-fast`,
  `cargo test -p fileferry-core`, `cargo test -p fileferry-cli`, `just fmt`,
  `just check`, `just test`, `just build`, and `git diff --check`.
- 2026-05-19 - Completed Milestone 5 key management first slice for
  initialized local repositories by implementing `ferry key add`. The command
  unlocks with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, accepts the
  new passphrase from `--new-password-file`, `FILEFERRY_NEW_PASSWORD`, or
  `FILEFERRY_NEW_PASSWORD_FILE`, and writes one immutable
  `key-slots/<key-slot-id>` object wrapping the existing repository master key.
  Added external key-slot discovery during repository unlock, a keyed
  master-key check for added slots, human/JSON/JSONL-safe CLI output, redacted
  failure envelopes, and documented that `key add` does not create a new
  master key, rewrite the original bootstrap slot, re-encrypt repository
  objects, recover lost keys, or implement S3-compatible key management.
  Verified with `cargo test -p fileferry-core repository_key_add`, `cargo test
  -p fileferry-cli key_add`, `cargo test -p fileferry-core`, `cargo test -p
  fileferry-cli`, `cargo test -p fileferry-crypto`, `just fmt`, `just check`,
  `just test`, and `just build`.
- 2026-05-19 - Completed Milestone 4 local backend interruption and
  corruption evidence for initialized local repositories. Added CLI coverage
  proving stale `.fileferry-tmp` files and malformed uncommitted partial
  objects do not affect normal snapshot discovery or check, plus Unix
  permission-denied backup JSON failure coverage when the platform exposes the
  denial. Added core coverage for immutable bootstrap write conflicts and
  CLI coverage for immutable conflict failure envelopes, and tightened local
  storage conflict cleanup assertions. Documented the tested evidence in
  `docs/operations.md` and `docs/cli-contract.md` without claiming repair,
  automatic stale temporary cleanup, prune, S3-compatible backup/restore/check,
  or platform-wide support. Verified with `cargo test -p fileferry-storage
  local_store_`, `cargo test -p fileferry-core
  repository_bootstrap_reports_immutable_write_conflicts`, targeted
  `fileferry-cli` local backend tests, `cargo test -p fileferry-storage`,
  `cargo test -p fileferry-core`, `cargo test -p fileferry-cli`, `just fmt`,
  `just check`, `just test`, `just build`, and `git diff --check`.
- 2026-05-19 - Completed Milestone 3 forget without prune for initialized
  local repositories. `ferry forget` now accepts retention keep flags
  (`--keep-last`, hourly/daily/weekly/monthly/yearly counts, and repeatable
  `--keep-tag`), supports `--dry-run`, evaluates selection through
  `fileferry-policy`, and writes immutable `forgets/<snapshot-id>` markers
  only when not in dry-run. Normal snapshot discovery ignores marked snapshots,
  but forget does not delete chunks, manifests, indexes, or commit objects and
  reports `object_deletion: false` in machine output. JSON and JSONL output
  include candidate, kept, and forgotten snapshot items with item-level
  reasons, dry-run status, marker counts, and stable no-match/invalid-policy
  exit behavior. Documented marker-only forget in `docs/cli-contract.md`,
  `docs/repository-format.md`, and README without claiming prune or
  S3-compatible forget support. Verified with `cargo test -p
  fileferry-policy`, `cargo test -p fileferry-core
  forget_markers_hide_snapshots_without_deleting_repository_objects`, `cargo
  test -p fileferry-cli forget_`, `cargo test -p fileferry-cli`, `cargo test
  -p fileferry-core`, `just fmt`, `just check`, `just test`, `just build`,
  and `git diff --check`.
- 2026-05-19 - Completed Milestone 2 S3-compatible init. `ferry init` now
  accepts `s3://bucket[/prefix]` repository URLs for encrypted repository
  bootstrap creation through `S3Store` wrapped in the common storage policy.
  S3 init requires explicit `FILEFERRY_S3_ENDPOINT`, `FILEFERRY_S3_REGION`,
  `FILEFERRY_S3_ACCESS_KEY_ID`, and `FILEFERRY_S3_SECRET_ACCESS_KEY`
  environment variables; credentials in repository URLs, query strings, and
  fragments are rejected. S3 repository URLs are redacted as
  `s3://<redacted>` in human, JSON, JSONL, and error output, and S3 config
  debug output keeps credentials redacted. Added unit coverage for S3 URL and
  environment parsing, CLI coverage for missing S3 environment and redaction,
  and a gated live CLI init test under `FILEFERRY_S3_INIT_INTEGRATION=1` that
  can only initialize a unique prefix below `FILEFERRY_S3_TEST_PREFIX`.
  Documented the CLI S3 init environment contract in `docs/cli-contract.md`,
  S3 storage notes, Backblaze B2 development notes, and README status without
  claiming S3 backup, restore, snapshots, ls, check, or v1 storage support.
  Verified with `cargo test -p fileferry-cli`, `just fmt`, `just check`,
  `just test`, `just build`, and `git diff --check`.
- 2026-05-19 - Completed Milestone 1 configurable check subsets for
  initialized local repositories. `ferry check` now accepts
  `--read-data-subset <N|PERCENT>`, validates counts and percentages as usage
  errors, authenticates all committed metadata before data reads, and selects
  deterministic referenced-chunk subsets from sorted chunk identities so the
  selected subset does not depend on object-store listing order. JSON and
  JSONL success output now report `read_data_mode: "subset"` and the requested
  `read_data_subset` for subset checks while full checks keep
  `read_data_mode: "full"` and `read_data_subset: null`; subset integrity
  failures still map to exit code `6`. Documented the implemented local check
  subset behavior in `docs/cli-contract.md` and updated README/BUILD status
  without adding S3, repair, doctor, or background-check claims. Verified with
  `cargo test -p fileferry-core check_repository_`, `cargo test -p
  fileferry-cli check_read_data_subset`, `cargo test -p fileferry-core -p
  fileferry-cli`, `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check`.
- 2026-05-19 - Reworked `BUILD.md` as a sharper execution queue for future
  agents without changing implementation status or checking off feature work.
  Added Active Milestones with definitions of done and non-goals for
  configurable check subsets, S3-compatible init, forget without prune, local
  backend interruption/corruption evidence, and the first key-management
  command slice. Added a Current Deprioritized Polish section so future passes
  prefer milestone completion over repeated small restore/check polish. Verified
  with `git diff --check`.
- 2026-05-19 - Tightened path-scoped Unix symlink restore and check identity
  diagnostics without expanding metadata or platform claims. Path-scoped
  symlink restores now create missing destination parent directories after
  destination safety preflight, matching regular-file parent handling while
  still rejecting existing symlink paths and symlinked ancestors.
  `fileferry-core` metadata identity mismatches now carry the repository object
  key, and `fileferry-cli` includes that key in JSON/JSONL check failure
  envelopes and `CheckFinding` details. Documented the path-scoped symlink
  behavior in `docs/cli-contract.md`. Verified initially with `cargo test -p
  fileferry-core path_scoped_symlink`, `cargo test -p fileferry-cli
  restore_path_scoped_symlink_creates_missing_parent_directory`, `cargo test -p
  fileferry-core repository_metadata_reads_reject_replayed_indexes_and_malformed_metadata`,
  and `cargo test -p fileferry-cli
  check_failure_finding_preserves_metadata_identity_object_key`; then with
  `cargo test -p fileferry-core -p fileferry-cli`, `just fmt`, `just check`,
  `just test`, `just build`, and `git diff --check`.
- 2026-05-19 - Tightened authenticated manifest validation before restore and
  check work without broadening repository format claims. `fileferry-core` now
  rejects decrypted manifests with invalid snapshot-relative entry paths,
  duplicate entry paths, non-file chunk references, regular-file
  size/chunk-length mismatches, or child entries whose recorded ancestor is not
  a directory. Restore rejects those manifests before destination writes, and
  `ferry check` reports them as integrity failures with snapshot id, manifest
  object key, and path context where available. `fileferry-cli` maps the new
  `snapshot_manifest_invalid` failure to exit code `6` and includes the
  context in check JSON/JSONL finding envelopes. Documented the behavior in
  `docs/cli-contract.md` and `docs/repository-format.md`. Verified initially
  with `cargo test -p fileferry-core invalid_manifest` and `cargo test -p
  fileferry-cli check_failure_finding_preserves`, then with `cargo test -p
  fileferry-core`, `cargo test -p fileferry-cli`, and the full `just fmt`,
  `just check`, `just test`, `just build`, and `git diff --check` gate.
- 2026-05-18 - Tightened restore destination safety without broadening restore
  scope. `fileferry-core` now preflights destination safety for all selected
  directories, regular files, and symlinks before any non-dry-run destination
  writes, so a later fail-if-exists conflict does not leave earlier selected
  entries behind. Added core and CLI regression coverage proving an existing
  destination file returns exit code `2` in JSON mode, keeps stderr empty, and
  leaves earlier selected directories unwritten. Documented the narrower
  restore guarantee in `docs/cli-contract.md`. Verified initially with
  targeted `cargo test -p fileferry-core
  restore_snapshot_to_destination_preflights_conflicts_before_writes` and
  `cargo test -p fileferry-cli
  restore_json_failure_preflights_destination_conflicts_before_writes`, then
  with `cargo test -p fileferry-core`, `cargo test -p fileferry-cli`, and the
  full `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check` gate.
- 2026-05-18 - Tightened `ferry check` integrity diagnostics for committed
  chunk-reference failures without adding repair or subset-check behavior.
  `fileferry-core` now carries snapshot id, snapshot-relative path, and
  object-key context through manifest/index chunk-reference mismatches and
  referenced chunk decompression failures when committed metadata provides
  that context. `fileferry-cli` maps those cases to integrity exit code `6`
  and includes the context in JSON/JSONL failure envelopes and `CheckFinding`
  details. Documented the narrower machine-output behavior in
  `docs/cli-contract.md`. Verified initially with targeted `cargo test -p
  fileferry-core ...` and `cargo test -p fileferry-cli ...` commands, then
  with `cargo test -p fileferry-core`, `cargo test -p fileferry-cli`, and the
  full `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check` gate.
- 2026-05-18 - Tightened restore and repository-incompatibility failure
  behavior without broadening command scope. `fileferry-core` now rejects any
  requested restore `--path` that matches no manifest entry before destination
  writes, and `fileferry-cli` reports that as `snapshot_path_not_found` with
  exit code `7` in machine output. Restore JSONL coverage now proves missing
  referenced chunks fail with an integrity envelope before destination writes.
  Unsupported repository format versions and declared repository features now
  have explicit core error classes and CLI failure codes mapped to the
  incompatible-repository exit family `3`, while malformed bootstrap JSON
  remains an integrity failure. Documented the behavior in
  `docs/cli-contract.md`. Verified initially with targeted `cargo test -p
  fileferry-core ...` and `cargo test -p fileferry-cli ...` commands, then
  with the full `just fmt`, `just check`, `just test`, `just build`, and
  `git diff --check` gate.
- 2026-05-18 - Tightened local repository open/check diagnostics without
  broadening command scope. `fileferry-core` now reports missing objects
  referenced by committed repository metadata as integrity failures outside
  `check` as well, and encrypted repository-object authentication failures now
  retain the object key for JSON/JSONL diagnostics and check findings.
  `fileferry-cli` maps those cases to stable machine-readable failure codes
  while preserving stdout/stderr separation and existing exit-code families.
  Added CLI integration coverage for uninitialized repositories, unsupported
  redacted S3 URLs, wrong passwords, corrupted bootstrap JSON, missing
  referenced manifests, malformed commit markers, and tampered encrypted
  metadata. Verified initially with `cargo test -p fileferry-core` and
  `cargo test -p fileferry-cli`.
- 2026-05-18 - Improved restore dry-run metadata reporting without broadening
  platform metadata claims. `fileferry-core` now reports `metadata_planned`
  for restored regular-file and directory modified timestamp fields and runs
  the same denied/unsupported/invalid timestamp planning checks during
  dry-run, while still applying only captured modified timestamps for regular
  files and directories during real restores. `fileferry-cli` includes
  `metadata_planned` in restore JSON/JSONL output, preserves warning behavior
  on stdout for machine modes, and documents the field in
  `docs/cli-contract.md` and `docs/platform-metadata.md`. Refreshed the local
  operations drill in `docs/operations.md` with observed modified-timestamp
  verification for one regular file and one nested directory. Verified
  initially with `cargo test -p fileferry-core` and `cargo test -p
  fileferry-cli`.
- 2026-05-18 - Added the first narrow restore metadata application slice for
  initialized local repositories. `fileferry-core` now carries captured
  modified timestamps through restore planning and applies them to restored
  regular files and directories after content writes; symlink timestamps,
  ownership, mode bits, ACLs, xattrs, resource forks, Windows attributes, BSD
  flags, and other platform-specific metadata remain unimplemented. Restore
  results now report `metadata_applied` from core, and metadata warning output
  uses partial-success exit code `10` while preserving JSON/JSONL stdout and
  stderr separation. Added core tests for successful file/directory timestamp
  application and warning generation, CLI integration coverage for restored
  file mtimes, and CLI unit coverage for partial-success warning output.
- 2026-05-18 - Improved `ferry check` corruption diagnostics and
  machine-readable failure behavior without adding repair or subset-check
  claims. Authenticated-object decode and metadata decode errors now retain the
  repository object key, and check reads map missing manifest/index objects
  referenced by repository metadata to integrity exit code `6` instead of a
  repository-not-found class. Runtime failures in `--json` and `--jsonl` modes
  now emit stable failure envelopes on stdout with `code`, `exit_code`,
  `retryable`, optional `path`, optional `object_key`, and `finding` details
  for check integrity failures; human mode still writes diagnostics to stderr.
  Added CLI integration tests for missing-chunk JSON failure and tampered-chunk
  JSONL failure with empty stderr, plus targeted core/CLI checks.
- 2026-05-18 - Expanded local restore and added the first `ferry check`
  implementation. Restore now writes explicit directory entries, regular-file
  contents, and Unix symlinks from initialized local repositories; it keeps
  destination containment checks, rejects symlinked destination ancestors and
  pre-existing symlink paths, creates symlinks after directory/file writes,
  reports `directories_written`, `symlinks_written`, `metadata_applied`, and
  `metadata_warnings` honestly, and still leaves metadata application
  unclaimed. Added core and CLI integration tests for directory/symlink
  restore success and symlink destination safety. `ferry check` now opens
  initialized local repositories, authenticates commit markers, encrypted
  manifests, encrypted indexes, and every referenced chunk, decompresses chunk
  payloads, verifies keyed chunk identities, and emits human, JSON, and JSONL
  output with `read_data_mode: "full"` and no configurable subset support.
  Added tests for wrong passwords, uninitialized repositories, missing chunks,
  tampered chunks, and JSON/JSONL success output. Ran a local
  `init -> backup -> restore -> check` drill against a temporary repository
  with an empty directory tree, a regular file, and a Unix symlink; verified
  file bytes with `cmp`, directory existence with `test -d`, symlink target
  with `readlink`, and check counts in JSON. Verified initially with targeted
  `cargo test -p fileferry-core ...` and `cargo test -p fileferry-cli ...`.
- 2026-05-18 - Wired `ferry restore` into `fileferry-cli` for initialized
  local repositories. The command unlocks through `FILEFERRY_PASSWORD` or
  `FILEFERRY_PASSWORD_FILE`, selects latest by default or via `--latest`,
  `--snapshot`, or `--tag`, accepts repeated snapshot-relative `--path`
  filters, restores regular-file contents through the existing core restore
  pipeline, verifies written bytes, supports `--dry-run`, and enforces
  fail-if-exists destination safety unless `--overwrite` is supplied. Added
  CLI integration tests for real `init -> backup -> restore` file-byte
  recovery, JSONL restore phases, wrong-password failure, and destination
  safety/overwrite behavior. Added `docs/operations.md` with a local restore
  drill performed against a temporary real FileFerry snapshot and byte-checked
  with `cmp`; no S3, metadata, directory-entry, or symlink restore coverage is
  claimed. Verified initially with `cargo test -p fileferry-cli`.
- 2026-05-18 - Wired `ferry backup` into `fileferry-cli` for initialized local
  repositories. The command accepts one or more local source paths plus repeated
  `--tag`, unlocks through `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`,
  runs the existing core backup pipeline, commits the snapshot, and emits
  human, JSON, and JSONL-safe output with backup summary fields and progress
  phases. Extended core snapshot write results with scanned entry counts, entry
  kind counts, scanned/uploaded byte counts, chunk seen/written/reused counts,
  and index ids. Added CLI integration tests that run `ferry init`, `ferry
  backup`, `ferry snapshots`, and `ferry ls` end to end against a real local
  repository, plus JSONL and wrong-password coverage. Verified initially with
  `cargo test -p fileferry-core -p fileferry-cli`; full gate passed with
  `just fmt`, `just check`, `just test`, `just build`, and `git diff --check`.
- 2026-05-18 - Wired the first end-user repository commands into
  `fileferry-cli`: `ferry init` now creates an encrypted local filesystem
  repository bootstrap from `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`,
  and `ferry snapshots` / `ferry ls` open initialized local repositories,
  authenticate committed encrypted manifests, and emit human, JSON, and
  JSONL-safe output. Added core bootstrap/open tests and CLI integration tests
  that initialize a real local repository, write a committed snapshot through
  the core pipeline, then list snapshots and entries through the `ferry`
  binary. S3-compatible repository bootstrap remains unchecked. Verified with
  `cargo test -p fileferry-core -p fileferry-cli`, `just fmt`, `just check`,
  `just test`, `just build`, and `git diff --check`.
- 2026-05-18 - Added committed snapshot discovery groundwork in
  `fileferry-core`: snapshot writes now publish a commit marker after chunk,
  index, and encrypted manifest objects; committed manifests can be discovered
  from commit markers; and tested snapshot summary plus immediate-entry listing
  helpers now support future `snapshots` and `ls` commands. Documented the
  plaintext commit marker fields in `docs/repository-format.md`. Added
  `docs/release.md` as the documented release equivalent until dedicated
  release tooling lands. Verified with `cargo test -p fileferry-core`.
- 2026-05-18 - Expanded `docs/cli-contract.md` with required v1 JSON document
  data schemas for `init`, `backup`, `restore`, `snapshots`, `ls`, `check`,
  `forget`, `prune`, `key` subcommands, and `version`, plus JSONL event data
  schemas and required long-operation phase names. Verified with `git diff
  --check` and `just check`.
- 2026-05-18 - Added destination restore primitives in `fileferry-core`:
  regular-file content can now be restored under an absolute destination root
  with path containment checks, symlinked-destination rejection, explicit
  fail-if-exists or overwrite behavior, dry-run reporting, and optional
  byte-for-byte verification. Verified with `cargo test -p fileferry-core`.
- 2026-05-18 - Added the first restore pipeline slice in `fileferry-core`:
  snapshot manifests now carry creation timestamps, loaded manifests can be
  selected by id, newest matching tag, or latest overall, and restore content
  reads path-scoped regular files back from encrypted chunks with chunk identity
  checks. Verified with `cargo test -p fileferry-core`.
- 2026-05-18 - Added authenticated repository-object read helpers for snapshot
  manifests and chunk indexes, including identity checks for decrypted metadata.
  Expanded adversarial coverage for wrong repository keys, bit flips,
  truncation, swapped objects, replayed indexes, and malformed metadata. Added
  backup-pipeline tests for sparse directory trees, symlinks, unreadable files,
  large files, many small files, and excluded paths. Verified with `cargo test
  -p fileferry-core -p fileferry-testkit`.
- 2026-05-18 - Added the first core backup pipeline slice: source entries are
  chunked, zstd-compressed, encrypted through the existing authenticated object
  envelope, written as immutable chunk objects, indexed in an encrypted chunk
  index, and recorded in an encrypted snapshot manifest. Chunk object names use
  keyed content identities so duplicate chunk content is represented once
  without leaking source paths in object names. Verified with `cargo test -p
  fileferry-core`.
- 2026-05-18 - Added validated FastCDC content-defined chunk planning in
  `fileferry-core`, including default v0 chunk-size targets, bounds checking
  against the FastCDC implementation, deterministic chunk-range tests, and
  small-input behavior. Marked the existing tested platform metadata capture
  Phase 5 item complete. Verified with `cargo test -p fileferry-core`.
- 2026-05-18 - Added initial backup-source walking in `fileferry-core` with
  deterministic traversal, absolute-root validation, wildcard exclusion rules
  including `**`, directory pruning, and symlink-aware metadata capture via
  `fileferry-platform`. Added initial portable metadata capture for entry kind,
  file size, timestamps, symlink targets, and Unix mode/ownership where
  available. Revisited the explicit config loader and kept it instead of adding
  `figment` or `config` because the current precedence model remains small and
  auditable. Verified with `cargo test -p fileferry-platform` and `cargo test
  -p fileferry-core`; full gate recorded with this change.
- 2026-05-18 - Added `PolicyObjectStore` and `StoragePolicy` so storage
  operations can be bounded by retry count, per-operation timeout,
  exponential backoff, and max concurrency. Added tests for policy validation,
  retryable failures, permanent conflict handling, timeouts, backoff capping,
  and concurrency limiting. Verified with `cargo test -p fileferry-storage`.
- 2026-05-18 - Added the `fileferry-web` public homepage crate for
  `fileferry.app`: Axum server, Leptos SSR marketing page, embedded CSS,
  `/healthz`, Ubuntu deployment notes, and route/render tests. Verified with
  `cargo test -p fileferry-web`; full gate recorded with this change.
- 2026-05-18 - Added the first `fileferry-policy` retention policy parser for
  count-based keep rules and repeated tag keep rules, documented current CLI
  JSON/JSONL schemas and data-mode progress behavior, and added
  `docs/platform-metadata.md` for v1 metadata capture and cross-platform
  restore reporting decisions. Verified initially with `cargo test -p
  fileferry-policy -p fileferry-cli`; full gate recorded with this change.
- 2026-05-18 - Completed the first Phase 3 slice: documented format v0
  security choices and repository-format structure, selected Argon2id,
  HKDF-SHA-256, and XChaCha20-Poly1305, implemented tested master-key
  creation/unlock, passphrase key slots, subkey derivation, and authenticated
  object envelopes in `fileferry-crypto`. Verified with `cargo test -p
  fileferry-crypto`.
- 2026-05-18 - Completed the first Phase 4 storage slice: added validated
  object keys, the object-store trait, storage capability reporting, a local
  filesystem backend with idempotent immutable writes, leftover temp-object
  listing protection, and an in-memory fake object store in
  `fileferry-testkit`. Added `docs/storage.md`. Verified with `cargo test -p
  fileferry-storage -p fileferry-testkit`.
- 2026-05-18 - Added the first real S3-compatible storage groundwork: an
  `object_store`-backed `S3Store`, HTTPS-only explicit S3 config, redacted
  credential handling, configurable conditional create support,
  prefix-scoped live integration test gate, Backblaze B2 development-bucket
  docs, and `.env` ignore rules. Verified with `just check`; the real
  Backblaze round-trip is gated on local S3 environment variables.
- 2026-05-18 - Completed the Phase 2 CLI foundation: config discovery,
  profiles, CLI/env/config precedence, typed config validation, redacted
  diagnostics, JSON and JSONL envelopes, event names, shell completions, and
  CLI golden tests. Added `docs/cli-contract.md`. Verified with `just check`.
- 2026-05-17 - Bootstrapped the Rust workspace with the planned crate
  boundaries, workspace dependency policy, `fileferry-cli` binary, basic
  `ferry version`, `just` recipes, and GitHub Actions CI. Verified with
  `just check`, individual `just fmt`/`just test`/`just build` recipes,
  direct `ferry version` smoke checks, and workflow YAML parsing.
- 2026-05-17 - Created the initial FileFerry planning docs:
  `README.md`, `BUILD.md`, and `AGENTS.md`.
