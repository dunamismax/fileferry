# Repository Format

FileFerry's repository format is original to FileFerry. Format v0's
compatibility contract is frozen for the object families and fields listed in
this document as of 2026-05-22. The freeze covers the current bootstrap/key-slot
and recovery-export objects, committed snapshot data, forget/prune state,
policy/config objects, upload state, lease state, migration detection gates,
object names, plaintext fields, encrypted-object framing, and AEAD
authentication context. Future fields, new object families, and migration
behavior require an explicit new format version or a documented feature gate
with fixtures.

## Format Principles

- Repository object names must not reveal source paths, file names, directory
  structure, tags, hostnames, usernames, or backup shape.
- Object storage is not POSIX. Correctness must not depend on rename,
  directory mutation, or immediate listing consistency.
- Writes should be immutable and retry-safe.
- Repository reads must authenticate encrypted objects before parsing
  plaintext.
- Prune must be two-phase and recoverable.
- Format migrations must be explicit, detectable, and tested.

## Plaintext Bootstrap

Only the following fields may be plaintext in format v0:

- Repository magic: identifies a FileFerry repository.
- Repository format version.
- Repository id: random public identifier used for key context binding.
- Supported algorithm ids for KDF and AEAD.
- Key-slot KDF parameters and salt.
- Key-slot AEAD nonce and encrypted master-key bytes.
- Optional non-sensitive feature flags required before unlock.

Reasons:

- The CLI needs to detect an uninitialized, unsupported, or incompatible
  repository before unlock.
- Passphrase unlock requires KDF parameters and salt before the master key is
  available.
- AEAD decrypt requires the nonce before plaintext exists.
- A random repository id lets keys and authentication contexts be bound to one
  repository without exposing backup contents.

No plaintext field may contain source paths, snapshot ids derived from
plaintext metadata, tags, hostnames, usernames, retention policy details, index
contents, chunk sizes tied to files, or object counts that are not already
visible from object storage listing.

Current implementation status:

- `ferry init` writes a plaintext `bootstrap` JSON object for local filesystem
  and S3-compatible repositories.
- The bootstrap object contains `magic`, `format_version`, a random
  32-byte repository id encoded as lowercase hex, key-slot KDF parameters,
  key-slot salt, key-slot AEAD nonce, encrypted master-key bytes, and an empty
  feature list.
- The key-slot fields are plaintext only to the extent required for
  passphrase unlock; the repository master key remains encrypted and
  authenticated.
- `ferry key add` for initialized local and S3-compatible repositories writes
  additional passphrase key slots as immutable `key-slots/<key-slot-id>` JSON
  objects. These objects contain the non-sensitive slot id, repository id,
  format version, KDF parameters, salt, nonce, encrypted master-key bytes, and
  a keyed master-key check used to reject slots that decrypt to a different
  repository master key. Added key slots do not rewrite existing repository
  objects or the original bootstrap key slot.
- `ferry key remove` for initialized local and S3-compatible repositories
  writes immutable `key-slot-removals/<key-slot-id>` JSON marker objects for
  externally added key slots. These markers contain the slot id, repository id,
  format version, target key-slot object name, removal timestamp, and a keyed
  removal check derived from the repository master key. Removal markers hide
  matching external key slots during unlock after the marker check verifies.
  They do not delete `key-slots/<key-slot-id>` objects and do not rewrite the
  bootstrap key slot or encrypted repository objects.
- `ferry key rotate` for initialized local and S3-compatible repositories is
  unlock rotation: it writes one new immutable `key-slots/<key-slot-id>` object
  for the existing repository master key, proves the new slot unlocks that
  master key, then writes immutable `key-slot-removals/<key-slot-id>` marker
  objects for explicitly selected externally added key slots. It does not
  create a new repository master key, delete key-slot objects, remove the
  original bootstrap key slot, retire unselected slots, or rewrite encrypted
  repository objects.
- `ferry key add`, `ferry key remove`, and `ferry key rotate` acquire encrypted
  `locks/<lease-id>` command lease state before key-slot mutation writes.
  Active readable leases reject the command as locked, and malformed lease
  state fails closed before key-slot objects or removal markers are written.
- `ferry key export-recovery --output <FILE>` for initialized local and
  S3-compatible repositories writes a standalone encrypted JSON recovery
  package outside the repository. The destination file must not already exist.
  The package contains only the minimum plaintext needed to identify and
  decrypt the package later: schema version, magic, format version, export
  type, repository id, random export id, creation time, warning text, KDF
  parameters and salt, AEAD algorithm and nonce, encrypted master-key bytes,
  and a keyed master-key check. The current implementation encrypts the
  package with the current repository passphrase, does not export raw
  master-key material, and does not rewrite repository objects.
- `ferry key import-recovery --input <FILE>` for initialized local and
  S3-compatible repositories reads a standalone encrypted recovery package,
  opens the target repository with the same passphrase, decrypts the package
  with that passphrase, verifies that the package repository id matches the
  target repository bootstrap, verifies the package master-key check against
  the opened repository master key, and writes one new immutable
  `key-slots/<key-slot-id>` object for the supplied new passphrase. It uses
  the same external key-slot layout as `ferry key add`, acquires encrypted
  `locks/<lease-id>` command lease state before writing the new key-slot
  object, does not create a new repository master key, and does not rewrite
  encrypted repository objects. The target-repository unlock requirement is
  deliberate for format v0 because bootstrap-only repositories do not contain
  an independent public master-key check.
- `ferry snapshots` and `ferry ls` read the bootstrap, unlock the master key
  from `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, then authenticate
  encrypted manifests before returning snapshot metadata.

## Object Name Layout

Object names are storage paths, not trusted metadata. They are opaque placement
keys and must be authenticated by object contents.

Format v0 object names:

```text
bootstrap
key-slots/<key-slot-id>
key-slot-removals/<key-slot-id>
objects/chunk/<prefix>/<random-or-content-id>
objects/manifest/<prefix>/<manifest-id>
objects/index/<prefix>/<index-id>
objects/policy/<prefix>/<policy-id>
objects/upload/<writer-id>/<upload-id>
objects/prune-plan/<prefix>/<plan-id>
objects/prune-completion/<prefix>/<plan-id>
commits/<commit-id>
locks/<lease-id>
forgets/<snapshot-id>
```

Rules:

- `<prefix>` is derived from the object id, not from source paths.
- Chunk ids and metadata ids must not be raw plaintext file hashes unless that
  hash is keyed or otherwise protected from offline path/content guessing.
- Object ids are authenticated inside encrypted metadata before use.
- Temporary upload names must include writer/upload randomness and no source
  path material.
- Listing object names may reveal approximate repository size; it must not
  reveal backed-up path names or tree shape.

## Authentication Context

Every encrypted object uses AEAD associated data. Format v0 object associated
data is:

```text
"fileferry\0format-v0\0object\0"
|| format_version
|| len(object_kind)
|| object_kind
|| len(object_name)
|| object_name
```

Format v0 object kinds:

- `chunk`
- `snapshot-manifest`
- `index`
- `policy-config`
- `repository-config`
- `upload-state`
- `prune-mark`
- `lease-state`

This binds ciphertext to its repository-format version, semantic object kind,
and storage name. Moving a ciphertext to a different name or opening it as a
different kind must fail authentication.

`repository-config` is a reserved v0 object kind for future encrypted
repository-local configuration. Current v0 code does not write or read a
repository-config object family, and no repository-config object layout is part
of the frozen v0 compatibility surface.

## Snapshot Manifest

Snapshot manifests are encrypted metadata objects. They describe a point-in-time
backup after source walking, chunking, compression, encryption, and index writes
have completed.

Frozen v0 manifest fields:

- Manifest schema version.
- Snapshot id.
- Creation timestamp.
- Source root, absolute source path, and snapshot-relative path records,
  encrypted at rest inside the manifest.
- Tags, encrypted at rest inside the manifest.
- File, directory, symlink, and other-entry records.
- Chunk references for regular file content.
- Platform metadata records or references, including the captured source
  platform for new manifests. Older v0 manifests without a source-platform
  field are treated as `unknown` by current readers.
- Index ids required to restore the snapshot.

Manifest records must not store plaintext paths, tags, usernames, hostnames, or
directory shape. Restore code must authenticate and parse the manifest before
presenting any decrypted metadata to the user.

Current format-v0 readers also validate decrypted manifest entry structure
before restore writes or check data reads. Entry paths must be normalized
snapshot-relative paths, duplicate entry paths are rejected, non-file entries
must not contain chunk references, captured regular-file sizes must match the
sum of referenced chunk lengths, and any recorded ancestor entry for a child
must be a directory. Violations are treated as integrity failures.

## Chunk Index

Chunk indexes are encrypted metadata objects that map chunk identities to object
locations and integrity data.

Frozen v0 index fields:

- Index schema version.
- Index id.
- Chunk id or keyed content id.
- Encrypted chunk object name.
- Plaintext, compressed, and stored lengths, encrypted at rest inside the
  index.
- Compression algorithm id.
- AEAD algorithm id.

Indexes are sensitive because they reveal deduplication and backup shape. They
remain encrypted and authenticated.

Current implementation status:

- The first core pipeline writes one encrypted index object per snapshot write.
- Chunk identities are keyed BLAKE3 values derived from the repository master
  key context, not raw plaintext hashes.
- Index object names are derived from keyed metadata identities and do not
  include source paths, tags, hostnames, or profile names.
- Current index readers reject unsupported decrypted index schema versions and
  metadata identity mismatches before check or restore uses the index.
- One committed snapshot-data fixture covers the current encrypted index object
  framing and decrypted index fields listed in the fixture-status section.
  Future pack membership requires a new schema/format decision and fixtures.

## Policy And Config Objects

Policy/config objects are encrypted metadata objects. They are intended for
repository-local policy state that would reveal backup shape or operator intent
if stored in plaintext.

Current implementation status:

- `fileferry-core` can write, list, read, and delete encrypted policy/config
  objects under `objects/policy/<prefix>/<policy-id>`.
- The current decrypted object contains `schema_version`, `magic`,
  `format_version`, `repository_id`, `policy_id`, and a `body` with
  `created_at_unix_seconds` and retention keep rules.
- Retention shape currently supports `keep_last`, `keep_hourly`, `keep_daily`,
  `keep_weekly`, `keep_monthly`, `keep_yearly`, and `keep_tags`. At least one
  keep rule is required, count rules must be greater than zero, and keep tags
  must not be empty or contain control characters.
- The policy id is a keyed content id over the encrypted body using the
  repository policy-config key purpose. Readers validate that the policy id,
  body identity, repository id, and object name agree before returning the
  object.
- Policy/config objects are encrypted and authenticated as object kind
  `policy-config` with the exact object name. Moving ciphertext to another
  object name or reading it as another object kind fails authentication.
- A repeated write of the same policy body is recognized as an idempotent
  already-present result. `ferry policy set` also treats an existing object
  with the same retention body as an idempotent CLI result instead of writing
  another timestamped policy body. `ferry policy show` lists authenticated
  policy/config objects after repository unlock. `ferry policy delete
  <POLICY_ID>` authenticates the selected policy object before deleting it and
  supports `--dry-run`.
- Stored policy configs are not automatically applied by `ferry forget` yet;
  current selection semantics are explicit by policy id for show/delete and by
  retention body for idempotent set.
- One policy/config fixture covers the current encrypted policy object framing
  and decrypted fields listed in the fixture-status section. Future config
  fields, policy selection semantics, or migration behavior require a new
  schema/format decision and fixtures.

## Commit Markers And Upload State

Backups publish a snapshot only after all referenced chunks, indexes, and the
manifest are durably written.

Format v0 commit model:

1. Write chunks and upload-state objects with retry-safe names.
2. Write indexes after their referenced chunks exist.
3. Write the encrypted manifest.
4. Write an immutable commit marker that references the manifest id.

Commit markers are small operational objects. They may contain only random ids,
format version, and encrypted or authenticated references needed to discover a
committed snapshot. They must not contain plaintext tags, paths, source counts,
or human names.

Current implementation status:

- The first backup pipeline writes `commits/<snapshot-id>` only after chunk,
  index, and encrypted manifest objects have been written.
- The commit marker is plaintext JSON containing `schema_version`,
  `snapshot_id`, and `manifest_object`.
- These plaintext fields are allowed because the same keyed snapshot id and
  manifest object key are already visible in repository object names. The marker
  adds no path, tag, source count, hostname, username, policy, or directory
  shape information.
- Commit marker contents are not trusted. Snapshot discovery validates that the
  marker key, snapshot id, and manifest object name agree, then authenticates
  and decrypts the encrypted manifest before returning snapshot metadata.
- `fileferry-core` can write and read encrypted upload-state objects under
  `objects/upload/<writer-id>/<upload-id>`. Writer and upload ids are random
  32-byte lowercase hex identifiers in current fixtures and do not contain
  source path material.
- The current decrypted upload-state object contains `schema_version`, `magic`,
  `format_version`, `repository_id`, `writer_id`, `upload_id`,
  `created_at_unix_seconds`, `operation`, the commit and forget-marker object
  sets observed when the state was marked, pending chunk/index/manifest object
  records, and a keyed `state_identity`.
- Upload-state objects are encrypted and authenticated as object kind
  `upload-state` with the exact object name. Moving ciphertext to another
  object name or reading it as another object kind fails authentication.
- Readers validate schema, magic, format version, repository id, writer id,
  upload id, object-key identity, pending object keys, and keyed state identity
  before returning the object.
- Resume reads compare the marked commit and forget-marker object sets with the
  current repository state and reject stale or replayed upload state when those
  sets no longer match. The current API is core-only; the backup pipeline does
  not yet use upload-state recovery.

## Forget Markers

Forget without prune is a state change only. It marks snapshots as no longer
visible to normal snapshot selection without deleting repository objects.

Current implementation status:

- `ferry forget` writes immutable `forgets/<snapshot-id>` marker objects for
  snapshots selected by the retention plan.
- The marker is plaintext JSON containing `schema_version`, `snapshot_id`,
  `manifest_object`, `commit_object`, and `forgotten_at_unix_seconds`.
- The plaintext ids and object keys are allowed because the same keyed snapshot
  id, manifest object key, and commit object key are already visible in
  repository object names. The marker adds no path, tag, source count,
  hostname, username, policy, or directory shape information.
- Marker contents are not trusted. Snapshot discovery validates that the marker
  key, snapshot id, manifest object name, and commit object name agree before a
  marker can hide a committed snapshot.
- Forget markers do not delete chunks, manifests, indexes, or commit markers.
  Storage reclamation requires the separate two-phase prune path.

Upload state records are encrypted. Interrupted uploads can be retried or
abandoned based on upload id and writer id. Correctness must not require
renaming a temporary object into place.

## Lock Or Lease Model

Concurrent backups are not claimed safe yet. Current backup writes are treated
as repository mutations and use the command lease path before publishing a
snapshot. Commands that mutate shared repository state, such as backup, forget,
prune, and key-slot changes, need a lease until narrower concurrency rules are
implemented and tested.

Format v0 lease rules:

- Leases are best-effort coordination, not the only protection against data
  loss.
- Lease objects use random ids and expiration timestamps.
- A writer must include command kind, writer id, start time, and expiration in
  encrypted or non-sensitive authenticated form.
- A stale lease can be broken only after its expiration and after a repository
  check confirms no required recovery action is pending.
- If backend capabilities cannot support the needed lease semantics, concurrent
  mutation must fail with a stable unsupported-capability error.

Current implementation status:

- `fileferry-core` can write and read one encrypted lease-state object under
  `locks/<lease-id>`.
- The current decrypted lease-state object contains `schema_version`, `magic`,
  `format_version`, `repository_id`, `lease_id`, `writer_id`, `command_kind`,
  `acquired_at_unix_seconds`, `expires_at_unix_seconds`, and a keyed
  `state_identity`.
- Lease-state objects are encrypted and authenticated as object kind
  `lease-state` with the exact object name. Moving ciphertext to another object
  name or reading it as another object kind fails authentication.
- Readers validate schema, magic, format version, repository id, lease id,
  writer id, object-key identity, expiration window, and keyed state identity
  before returning the object. Active-use reads reject an expired lease.
- `ferry backup`, non-dry-run `ferry forget`, non-dry-run `ferry prune`, and
  key-management mutation paths now use this lease-state format for
  command-level coordination. Before writing snapshot objects, forget markers,
  marking/sweeping a prune plan, writing key-slot objects, or writing key-slot
  removal markers, the command lists `locks/`, authenticates and validates
  readable lease state, rejects another active lease, ignores expired readable
  leases, writes its own encrypted lease, and rechecks active leases after the
  write. When the mutation path returns, the command best-effort deletes its own
  lease; if release is interrupted, timestamp expiry is the fallback.
- Lease enforcement is currently proven for `ferry backup`, non-dry-run forget,
  non-dry-run prune, `ferry key add`, `ferry key remove`, and `ferry key
  rotate`. Dry-run forget and dry-run prune write no lease state. Command-level
  leases for repository maintenance, stale-lease breaking, and lease repair are
  not implemented yet.

## Concurrent Backup Behavior

Future concurrent backups may be allowed when:

- They do not share mutable upload ids.
- They write immutable chunks, indexes, manifests, and commit markers.
- Deduplication races are handled by idempotent object writes.
- Commit discovery tolerates out-of-order listing.

Current `ferry backup` takes the repository mutation lease before scanning and
publishing a snapshot. It rejects another active readable lease and malformed
lease state before writing chunk, index, manifest, or commit objects. This is a
narrow pre-v1 safety rule, not a final concurrent-backup design.

Concurrent backup must be rejected when the backend cannot provide the minimum
idempotent write and visibility behavior needed for safe publication.

Backup and prune must not run concurrently unless prune can prove the backup's
committed or in-progress objects are protected from deletion.

## Prune Mark, Sweep, And Recovery

Current local and S3-compatible prune is two-phase:

1. Mark: compute a prune plan and write one encrypted prune-plan object under
   `objects/prune-plan/<prefix>/<plan-id>`. The plan identifies candidate
   objects, retained objects, observed commit-marker keys, observed
   forget-marker keys, byte counts where known, and plan metadata.
2. Sweep: delete only candidate objects named by the marked plan after
   checking that the current commit/forget marker state still matches the
   marked state, ignoring candidate commit/forget markers already deleted by
   that same plan. After all candidates are deleted or observed missing,
   write one encrypted completion object under
   `objects/prune-completion/<prefix>/<plan-id>`.

Current implementation status:

- `ferry prune` is implemented for initialized local filesystem and
  S3-compatible repositories through the shared encrypted object-store
  pipeline.
- Backup, non-dry-run forget, and non-dry-run prune write an encrypted
  `locks/<lease-id>` command lease before shared repository mutation. Another
  active readable lease rejects the command as a locked repository, expired
  readable leases are ignored, and malformed lease state fails closed before
  backup writes snapshot objects, forget writes markers, or prune deletes
  candidates.
- Candidate objects are limited to commit markers, forget markers, encrypted
  manifests, encrypted indexes, and encrypted chunks reachable from forgotten
  committed snapshots and not reachable from non-forgotten committed
  snapshots.
- Prune never deletes `bootstrap`, `key-slots/`, `key-slot-removals/`,
  `objects/policy/`, `objects/upload/`, `locks/`, prune state, or unknown
  objects.
- Prune plan and completion objects are encrypted and authenticated as
  `prune-mark` objects with the repository prune subkey.
- Current readers validate prune plan and prune completion schema, magic,
  repository id, format version, object-name identity, and plan identity before
  using prune state for recovery decisions.
- S3-compatible prune relies on immutable prune state, idempotent object
  deletes, and prefix listing. It does not require rename operations for
  correctness and does not use provider-specific lifecycle policies.

Recovery rules:

- A mark without a completed sweep is recoverable by rechecking live commits and
  resuming the plan when the observed commit/forget marker state still matches.
- A sweep must be idempotent; missing candidate objects are recorded as already
  gone, not fatal corruption by themselves.
- If the observed commit/forget marker state has changed, the current local
  implementation aborts the sweep instead of deleting from a stale plan.
- Plan expiration and explicit abandonment are not implemented yet.
- Dry-run prune uses the same reachability logic but writes no marks and deletes
  nothing.

## Migrations

Every repository has a detectable format version. Future migrations must:

- Refuse unknown future versions.
- Explain unsupported older versions with a stable error.
- Record whether migration is read-compatible, write-compatible, or requires a
  one-way rewrite.
- Have tests for old fixtures before the migration is advertised.

Current implementation status:

- `fileferry-core` can inspect the plaintext bootstrap format version and
  declared feature flags without unlocking repository key slots.
- Format version `0` with no feature flags is classified as the current
  supported repository format.
- Unknown future format versions are classified as future-incompatible and are
  rejected before unlock with `UnsupportedRepositoryFormat`.
- Format version `0` with unknown feature flags is classified as
  feature-incompatible and is rejected before unlock with
  `UnsupportedRepositoryFeatures`.
- Unversioned bootstrap objects are treated as invalid pre-v0/unversioned
  repository metadata. No migration path exists for them.
- No read-compatible, write-compatible, or one-way migration is implemented
  yet.

## Fixture Status

Golden fixtures exist under:

- `tests/fixtures/repository-format/v0/bootstrap-keyslot/` for the bootstrap
  object, one external key-slot object, and one key-slot removal marker.
- `tests/fixtures/repository-format/v0/recovery-export/` for one standalone
  encrypted recovery export package.
- `tests/fixtures/repository-format/v0/snapshot-data/` for one initialized
  repository containing one plaintext commit marker, one encrypted snapshot
  manifest, one encrypted chunk index, and two encrypted chunk objects.
- `tests/fixtures/repository-format/v0/forget-prune-state/` for one initialized
  repository after a prune sweep, plus the captured forget marker that the
  sweep deleted. The fixture covers one plaintext forget marker, one encrypted
  prune plan, one encrypted prune completion object, and the retained snapshot
  data needed to check the post-prune repository.
- `tests/fixtures/repository-format/v0/policy-config/` for one initialized
  repository containing one encrypted policy/config object.
- `tests/fixtures/repository-format/v0/upload-state/` for one initialized
  repository containing one encrypted upload-state object.
- `tests/fixtures/repository-format/v0/lease-state/` for one initialized
  repository containing one encrypted lease-state object.
- `tests/fixtures/repository-format/v0/migration/` for bootstrap-only
  compatibility-gate fixtures covering current v0 detection, an unsupported
  future format version, unsupported v0 feature flags, and an unversioned
  pre-v0 rejection case.

These fixtures are the format v0 compatibility contract for the object families
they cover. They do not implement migrations or promise cross-version
read/write compatibility beyond detecting unsupported bootstrap versions and
feature flags before unlock.

For fixture-covered current v0 objects, the listed compatibility-facing fields
below are closed schemas: current readers reject unknown JSON fields in the
plaintext object, encrypted-object frame, or decrypted repository metadata
before the object can influence repository behavior. The bootstrap inspection
gate intentionally remains narrower and looser: it reads only `magic`,
`format_version`, and `features` so future or feature-gated formats can be
classified before unlock. If inspection classifies bytes as the current
supported format, the full bootstrap decode uses the strict current-v0 schema.

Fields not listed below are not part of frozen format v0. This includes future
manifest body additions, future index packing fields, policy selection
semantics, upload recovery semantics, lease breaking/repair semantics, prune
abandonment/expiration semantics, and migration behavior beyond the bootstrap
compatibility gate. Adding one of those fields to repositories that current v0
readers must open requires a new schema/format decision, a compatibility plan,
and fixtures. Platform metadata extension fields are governed by
`docs/platform-metadata.md`; the fixture-covered manifest freezes the metadata
fields present in that fixture and rejects unknown metadata, extension, and
metadata-summary fields.

For the bootstrap/key-slot fixture slice, the compatibility-facing fields are:

- `bootstrap`: `magic`, `format_version`, `repository_id`, `key_slots`, each
  embedded key-slot KDF field, salt, nonce, wrapped master-key bytes, and
  `features`.
- `key-slots/<key-slot-id>`: `magic`, `format_version`, `repository_id`,
  `key_slot_id`, the nested key-slot KDF fields, salt, nonce, wrapped
  master-key bytes, and `master_key_check`.
- `key-slot-removals/<key-slot-id>`: `schema_version`, `magic`,
  `format_version`, `repository_id`, `key_slot_id`, `key_slot_object`,
  `removed_at_unix_seconds`, and `master_key_removal_check`.
- `recovery-export/recovery.fileferry-key`: `schema_version`, `magic`,
  `format_version`, `export_type`, `repository_id`, `export_id`,
  `created_at_unix_seconds`, `warning`, `aead`, each nested recovery-key KDF
  field, salt, nonce, wrapped master-key bytes, and `master_key_check`.
  Current readers accept the legacy pre-import warning text in the frozen
  fixture as well as the current warning text written by new exports.
- `snapshot-data/commits/<snapshot-id>`: `schema_version`, `snapshot_id`, and
  `manifest_object`.
- `snapshot-data/objects/manifest/<prefix>/<snapshot-id>`: encrypted-object
  `algorithm`, `nonce`, and `ciphertext`, authenticated as object kind
  `snapshot-manifest` and the exact object name; decrypted `schema_version`,
  `snapshot_id`, `body.created_at_unix_seconds`, `body.tags`, `body.entries`,
  entry path fields, captured metadata fields present in the fixture,
  `body.entries[].chunks`, and `body.index_ids`.
- `snapshot-data/objects/index/<prefix>/<index-id>`: encrypted-object
  `algorithm`, `nonce`, and `ciphertext`, authenticated as object kind `index`
  and the exact object name; decrypted `schema_version`, `index_id`, and each
  chunk entry's `chunk_id`, `object_key`, `plaintext_length`,
  `compressed_length`, `stored_length`, `compression`, and `aead`.
- `snapshot-data/objects/chunk/<prefix>/<chunk-id>`: encrypted-object
  `algorithm`, `nonce`, and `ciphertext`, authenticated as object kind `chunk`
  and the exact object name. The decrypted bytes are zstd-compressed chunk
  payloads whose decompressed bytes must match the keyed chunk identity recorded
  in the manifest and index.
- `forget-prune-state/forgets/<snapshot-id>`: `schema_version`, `snapshot_id`,
  `manifest_object`, `commit_object`, and `forgotten_at_unix_seconds`.
- `forget-prune-state/objects/prune-plan/<prefix>/<plan-id>`: encrypted-object
  `algorithm`, `nonce`, and `ciphertext`, authenticated as object kind
  `prune-mark` and the exact object name; decrypted `schema_version`, `magic`,
  `format_version`, `repository_id`, `plan_id`, `created_at_unix_seconds`,
  `commit_objects`, `forget_marker_objects`, `candidate_objects`, and
  `retained_objects`.
- `forget-prune-state/objects/prune-completion/<prefix>/<plan-id>`:
  encrypted-object `algorithm`, `nonce`, and `ciphertext`, authenticated as
  object kind `prune-mark` and the exact object name; decrypted
  `schema_version`, `magic`, `format_version`, `repository_id`, `plan_id`,
  `plan_object`, `completed_at_unix_seconds`, object counts, and byte counts.
- `policy-config/objects/policy/<prefix>/<policy-id>`: encrypted-object
  `algorithm`, `nonce`, and `ciphertext`, authenticated as object kind
  `policy-config` and the exact object name; decrypted `schema_version`,
  `magic`, `format_version`, `repository_id`, `policy_id`,
  `body.created_at_unix_seconds`, and retention fields `keep_last`,
  `keep_hourly`, `keep_daily`, `keep_weekly`, `keep_monthly`, `keep_yearly`,
  and `keep_tags`.
- `upload-state/objects/upload/<writer-id>/<upload-id>`: encrypted-object
  `algorithm`, `nonce`, and `ciphertext`, authenticated as object kind
  `upload-state` and the exact object name; decrypted `schema_version`,
  `magic`, `format_version`, `repository_id`, `writer_id`, `upload_id`,
  `created_at_unix_seconds`, `operation`, `commit_objects`,
  `forget_marker_objects`, `pending_objects`, and `state_identity`.
- `lease-state/locks/<lease-id>`: encrypted-object `algorithm`, `nonce`, and
  `ciphertext`, authenticated as object kind `lease-state` and the exact
  object name; decrypted `schema_version`, `magic`, `format_version`,
  `repository_id`, `lease_id`, `writer_id`, `command_kind`,
  `acquired_at_unix_seconds`, `expires_at_unix_seconds`, and `state_identity`.
- `migration/future-format-bootstrap/bootstrap`: plaintext `magic`,
  `format_version`, `repository_id`, `key_slots`, and `features`; the fixture
  proves that unknown future repository formats are detected without unlock and
  rejected as incompatible.
- `migration/future-feature-bootstrap/bootstrap`: plaintext `magic`,
  `format_version`, `repository_id`, `key_slots`, and `features`; the fixture
  proves that unknown feature flags in the current format are detected without
  unlock and rejected as incompatible.
- `migration/unversioned-bootstrap/bootstrap`: plaintext `magic`,
  `repository_id`, `key_slots`, and `features`; the fixture proves that
  unversioned pre-v0 bootstrap metadata has no implicit migration path and is
  rejected as invalid bootstrap metadata.

The fixture passphrases are test-only unlock inputs. They are not production
secrets. Tests prove current code can read the bootstrap/key-slot fixture,
reject malformed bootstrap JSON, reject unsupported bootstrap versions, reject
a tampered external key-slot master-key check, reject a tampered key-slot
removal marker check, and fail closed when a removed key-slot passphrase is
used. Tests also prove current code can verify the recovery export fixture,
reject malformed recovery-export JSON, reject unsupported recovery-export
format versions, reject tampered recovery-export ciphertext, reject a tampered
recovery-export master-key check, and import valid recovery exports as new
external key slots only after repository-id and master-key checks pass. The
snapshot-data fixture tests prove current code can unlock the fixture, read
committed manifests, authenticate and validate the encrypted manifest and
index, run full repository check across referenced chunks, restore selected
file bytes, reject malformed commit JSON, reject malformed encrypted-object
framing and decrypted manifest metadata, reject encrypted manifest/index/chunk
tampering, reject wrong object names and kinds through AEAD context binding,
reject manifest and index metadata identity mismatches, and reject unsupported
commit, manifest, and index schema versions.
The forget/prune-state fixture tests prove current code can unlock the fixture,
read the forget marker, authenticate and validate prune plan and completion
state, check the retained post-prune snapshot data, reject malformed forget
marker JSON, reject forget marker schema and identity mismatches, reject
malformed prune encrypted-object framing and decrypted metadata, reject prune
plan and completion ciphertext tampering, reject wrong object names and
authenticated kinds through AEAD context binding, reject prune metadata identity
mismatches, reject unsupported prune schemas and format versions, reject a
tampered completion object during prune recovery scanning, and reject stale
pending prune-plan replay when current commit/forget marker state no longer
matches the marked plan.
The policy/config fixture tests prove current code can unlock the fixture,
authenticate and validate the encrypted policy/config object, idempotently
recognize an already-present matching policy body, reject malformed encrypted
framing and decrypted metadata, reject policy ciphertext tampering, reject wrong
object names and authenticated kinds through AEAD context binding, reject policy
metadata identity mismatches, reject unsupported policy/config schema and format
versions, reject repository identity mismatches, and reject invalid retention
shape.
The upload-state fixture tests prove current code can unlock the fixture,
authenticate and validate the encrypted upload-state object, idempotently
recognize an already-present matching upload state, reject malformed encrypted
framing and decrypted metadata, reject upload-state ciphertext tampering, reject
wrong object names and authenticated kinds through AEAD context binding, reject
upload-state metadata identity mismatches, reject unsupported upload-state
schema and format versions, reject repository identity mismatches, and reject
stale upload-state replay when current commit/forget marker state no longer
matches the marked state.
The migration fixture tests prove current code can inspect current v0
bootstrap metadata without unlock, open the current v0 fixture, reject
malformed bootstrap JSON during inspection, reject unknown future format
versions before unlock, reject unknown current-format feature flags before
unlock, and reject unversioned pre-v0 bootstrap metadata as invalid rather than
guessing a migration.
The lease-state fixture tests prove current code can unlock the fixture,
authenticate and validate the encrypted lease-state object, idempotently
recognize an already-present matching lease state, reject malformed encrypted
framing and decrypted metadata, reject lease-state ciphertext tampering, reject
wrong object names and authenticated kinds through AEAD context binding, reject
lease metadata identity mismatches, reject unsupported lease-state schema and
format versions, reject repository identity mismatches, reject invalid
expiration windows, and reject expired leases for active use. Focused prune
tests now prove non-dry-run prune rejects an active lease before marking or
deleting, ignores an expired readable lease, best-effort releases its own lease
after a completed sweep, and rejects malformed lease state before deleting
candidate objects. Focused forget tests now prove non-dry-run forget rejects
an active lease before writing markers, ignores an expired readable lease,
best-effort releases its own lease after successful marker writes, rejects
malformed lease state before writing markers, and keeps dry-run forget
lease-free. Focused key-management tests now prove `key add`, `key remove`,
and `key rotate` reject active or malformed lease state before key-slot object
or key-slot removal-marker writes, that the shared key-management lease path
ignores expired readable leases, and that successful key-management mutations
release their own leases. Focused backup tests now prove `ferry backup` uses
the shared lease path, rejects an active readable lease before writing snapshot
objects, ignores an expired readable lease, best-effort releases its own lease
after a successful snapshot write, and rejects malformed lease state before
writing snapshot objects.

Current compatibility-contract strictness tests prove that fixture-covered
current v0 objects reject unknown fields in bootstrap bytes, external key-slot
objects, nested key-slot KDF parameters, key-slot removal markers, recovery
exports, encrypted-object frames, decrypted manifests, manifest entry metadata,
platform metadata extension summaries, chunk-index entries, commit markers,
forget markers, prune plans, prune completions, policy/config retention bodies,
upload pending-object records, and lease-state objects. These tests freeze the
listed v0 compatibility surface; adding a field now requires an intentional
schema change, fixture update, and migration or compatibility decision.
