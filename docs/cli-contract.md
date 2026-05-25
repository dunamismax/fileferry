# CLI Contract

FileFerry's command-line interface is intended for scripts first. Human output
may improve across compatible releases, but stdout/stderr separation, exit
code families, and machine-output envelopes are treated as compatibility
surfaces once marked stable.

## Streams

- Stdout is data.
- Stderr is diagnostics, logs, and progress.
- `--json` writes exactly one JSON document to stdout.
- `--jsonl` writes one JSON event per line to stdout.
- Completion scripts are stdout data and are not wrapped in JSON or JSONL.
- Progress UI must never be written to stdout in JSON or JSONL modes.

## Exit Codes

These exit code families are stable for the current CLI foundation:

```text
0   success
1   generic failure or internal serialization/completion failure
2   invalid command, arguments, config, or environment
3   repository not found, uninitialized, locked, or incompatible
4   authentication, password, key, or permission failure
5   storage, network, or filesystem I/O failure
6   integrity, corruption, tampering, or verification failure
7   requested snapshot, path, tag, or policy was not found
8   operation was interrupted after reaching a safe state
9   unsupported platform, filesystem feature, or backend capability
10  partial success; inspect JSON output for item-level failures
```

The current implementation can emit these families for the implemented command
surface: `0`, `1`, `2`, `3`, `4`, `5`, `6`, `7`, `8`, `9`, and `10`.
Unsupported repository format versions and declared repository features are
reported as incompatible repository failures in family `3`, not as corruption
or tampering failures.

The current code has regression coverage for the exit-code family mapping,
JSON and JSONL envelopes, JSONL event order, and stdout/stderr separation for
machine-output modes. Argument parsing failures from `clap` remain usage
diagnostics on stderr and are not JSON-wrapped.

## Global Precedence

Configuration is resolved in this order:

```text
CLI flags > environment variables > selected config profile > root config > defaults
```

Implemented environment variables:

```text
FILEFERRY_CONFIG
FILEFERRY_PROFILE
FILEFERRY_REPOSITORY
FILEFERRY_PASSWORD
FILEFERRY_PASSWORD_FILE
FILEFERRY_NEW_PASSWORD
FILEFERRY_NEW_PASSWORD_FILE
FILEFERRY_S3_ENDPOINT
FILEFERRY_S3_REGION
FILEFERRY_S3_ACCESS_KEY_ID
FILEFERRY_S3_SECRET_ACCESS_KEY
FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE
FILEFERRY_S3_ALLOW_INSECURE_HTTP
FILEFERRY_LOG
```

Runtime diagnostics must not print passphrase values, S3 credential values, or
full secret-bearing environment dumps. Usage docs may name the supported
environment variables, but password-missing runtime errors use generic
passphrase wording instead of printing the secret-bearing password environment
variable names. Config parser diagnostics redact embedded URL userinfo,
queries, and fragments before they are emitted in human, JSON, or JSONL
failure output.

When no config path is supplied by `--config` or `FILEFERRY_CONFIG`, FileFerry
looks for `fileferry.toml` and then `.fileferry.toml` in the current working
directory.

## JSON Document Envelope

`--json` emits one document:

```json
{
  "schema_version": 1,
  "command": "version",
  "status": "success",
  "data": {
    "command": "ferry",
    "version": "1.0.0"
  }
}
```

`status` is `success` for completed commands. Future commands may add
command-specific fields under `data` without changing the envelope.
Runtime failures after CLI parsing use `status: "failure"` and the
`command_failed` data schema documented below. Argument parsing errors from
`clap` are still emitted as normal usage diagnostics.

Current schema:

```text
CommandDocument<T>
  schema_version: integer, currently 1
  command: string
  status: "success" | "failure"
  data: T
```

Required v1 command document data schemas:

```text
init
  data.repository_id: string
  data.repository_url: redacted string
  data.format_version: integer
  data.backend: "local" | "s3_compatible"
  data.created: boolean
  data.key_slots: integer

backup
  data.snapshot_id: string
  data.repository_id: string
  data.started_at_unix_seconds: integer
  data.completed_at_unix_seconds: integer
  data.sources: array of redacted strings
  data.tags: array of strings
  data.entries_scanned: integer
  data.files_backed_up: integer
  data.directories_backed_up: integer
  data.symlinks_backed_up: integer
  data.special_entries_seen: integer
  data.bytes_scanned: integer
  data.bytes_uploaded: integer
  data.chunks_seen: integer
  data.chunks_written: integer
  data.chunks_reused: integer
  data.index_ids: array of strings
  data.manifest_id: string

restore
  data.snapshot_id: string
  data.destination: redacted string
  data.paths: array of snapshot-relative strings
  data.dry_run: boolean
  data.overwrite: "fail_if_exists" | "overwrite_files"
  data.entries_selected: integer
  data.files_written: integer
  data.directories_written: integer
  data.symlinks_written: integer
  data.metadata_planned: integer
  data.metadata_applied: integer
  data.metadata_warnings: array of RestoreMetadataWarning
  data.bytes_written: integer
  data.verified_files: integer

snapshots
  data.snapshots: array of SnapshotSummary

ls
  data.snapshot_id: string
  data.path: snapshot-relative string
  data.entries: array of SnapshotEntry

find
  data.snapshots_searched: integer
  data.matches_count: integer
  data.matches: array of SnapshotFindMatch

diff
  data.from_snapshot_id: string
  data.to_snapshot_id: string
  data.path_scopes: array of snapshot-relative strings
  data.added_count: integer
  data.removed_count: integer
  data.changed_count: integer
  data.unchanged_count: integer
  data.entries: array of SnapshotDiffEntry

repo
  data.repository_url: redacted string
  data.backend: "local" | "s3_compatible"
  data.initialized: boolean
  data.repository_id: string | null
  data.format: RepositoryFormatStatus | null
  data.storage: RepositoryStorageStatus
  data.object_families: array of RepositoryObjectFamilyStatus
  data.verification: RepositoryVerificationStatus | null

doctor
  data.repository_url: redacted string
  data.backend: "local" | "s3_compatible"
  data.initialized: boolean
  data.repository_id: string | null
  data.format: RepositoryFormatStatus | null
  data.storage: RepositoryStorageStatus
  data.health.status: "healthy" | "uninitialized" | "incompatible" |
                      "degraded"
  data.health.checked: boolean
  data.health.diagnostics: array of safe diagnostic strings
  data.verification: RepositoryVerificationStatus | null
  data.object_families: array of RepositoryObjectFamilyStatus | null
  data.repair.attempted: boolean
  data.repair.available: boolean

check
  data.repository_id: string
  data.checked_at_unix_seconds: integer
  data.metadata_objects_checked: integer
  data.chunk_objects_checked: integer
  data.bytes_read: integer
  data.read_data_mode: "metadata_only" | "subset" | "full"
  data.read_data_subset: string | null
  data.errors: array of CheckFinding
  data.warnings: array of CheckFinding

forget
  data.dry_run: boolean
  data.policy_source: "inline" | "stored"
  data.policy_id: string | null
  data.snapshots_matched: integer
  data.snapshots_forgotten: integer
  data.retained_snapshots: integer
  data.object_deletion: boolean
  data.marker_objects_written: integer
  data.candidate_snapshots: array of ForgetSnapshotItem
  data.kept_snapshots: array of ForgetSnapshotItem
  data.forgotten_snapshots: array of ForgetSnapshotItem
  data.forgotten_snapshot_ids: array of strings
  data.policy_summary: RetentionPolicySummary

prune
  data.repository_id: string
  data.plan_id: string | null
  data.dry_run: boolean
  data.resumed: boolean
  data.completed: boolean
  data.recovery_state: "dry_run" | "marked" | "resumed" | "completed"
  data.candidate_objects: array of PruneObject
  data.retained_objects: array of PruneObject
  data.deleted_objects: array of PruneObject
  data.missing_objects: array of PruneObject
  data.candidate_object_count: integer
  data.retained_object_count: integer
  data.deleted_object_count: integer
  data.missing_object_count: integer
  data.candidate_bytes: integer
  data.retained_bytes: integer
  data.deleted_bytes: integer
  data.missing_bytes: integer

policy set
  data.repository_id: string
  data.policy_id: string
  data.policy_object: repository object key string
  data.created_at_unix_seconds: integer
  data.retention: RetentionPolicySummary
  data.created: boolean
  data.bytes_written: integer
  data.encrypted_at_rest: boolean
  data.applied_to_forget: boolean

policy show
  data.repository_id: string
  data.policy_count: integer
  data.policies: array of PolicyConfigItem

policy delete
  data.repository_id: string
  data.policy_id: string
  data.policy_object: repository object key string
  data.created_at_unix_seconds: integer
  data.retention: RetentionPolicySummary
  data.dry_run: boolean
  data.deleted: boolean

key add
  data.repository_id: string
  data.key_slot_id: string
  data.key_slots: integer
  data.kdf: KdfSummary
  data.reencrypted_repository_objects: boolean

key remove
  data.repository_id: string
  data.removed_key_slot_id: string
  data.key_slots: integer
  data.removal_marker_object: string
  data.removal_marker_created: boolean
  data.deleted_key_slot_objects: boolean
  data.reencrypted_repository_objects: boolean

key rotate
  data.repository_id: string
  data.added_key_slot_id: string
  data.removed_key_slot_ids: array of strings
  data.key_slots: integer
  data.removal_marker_objects: array of strings
  data.removal_markers_created: integer
  data.kdf: KdfSummary
  data.deleted_key_slot_objects: boolean
  data.reencrypted_repository_objects: boolean

key rekey
  data.repository_id: string
  data.rekey_id: string
  data.old_key_slots: integer
  data.new_key_slots: integer
  data.snapshots_rewritten: integer
  data.chunks_rewritten: integer
  data.indexes_rewritten: integer
  data.manifests_rewritten: integer
  data.commits_rewritten: integer
  data.forget_markers_rewritten: integer
  data.policies_rewritten: integer
  data.upload_states_rewritten: integer
  data.prune_states_rewritten: integer
  data.leases_removed: integer
  data.old_key_slots_retired: integer
  data.old_key_slot_objects_deleted: integer
  data.old_repository_objects_deleted: integer
  data.recovery_state: "started" | "resumed"
  data.kdf: KdfSummary
  data.old_unlocks_retained: boolean
  data.raw_master_key_exported: boolean
  data.reencrypted_repository_objects: boolean

key export-recovery
  data.repository_id: string
  data.export_id: string
  data.destination: redacted string
  data.key_slots: integer
  data.created_at_unix_seconds: integer
  data.kdf: KdfSummary
  data.aead: "xchacha20_poly1305"
  data.warning: string
  data.recovery_import_implemented: boolean
  data.raw_master_key_exported: boolean
  data.reencrypted_repository_objects: boolean

key import-recovery
  data.repository_id: string
  data.export_id: string
  data.source: redacted string
  data.added_key_slot_id: string
  data.key_slots: integer
  data.created_at_unix_seconds: integer
  data.kdf: KdfSummary
  data.aead: "xchacha20_poly1305"
  data.raw_master_key_exported: boolean
  data.reencrypted_repository_objects: boolean

version
  data.command: "ferry"
  data.version: semantic-version string from the package version
```

Shared data records:

```text
SnapshotSummary
  snapshot_id: string
  created_at_unix_seconds: integer
  tags: array of strings
  source_count: integer
  entry_count: integer

RestoreMetadataWarning
  entry_id: snapshot entry identifier; currently the snapshot-relative path
  path: snapshot-relative string
  namespace: metadata namespace, for example "portable", "unix", or the
             source platform namespace for platform-extension status warnings
  field: string; current portable timestamp fields are "modified" for
         regular-file and directory modified-time restore warnings, and
         "created" for warning-only creation/birth timestamps when they were
         selected from captured metadata. Accessed timestamps are not captured
         or reported by this version.
  source_platform: "windows" | "macos" | "linux" | "unix" | "unknown"
  destination_platform: same values as source_platform
  reason: string

SnapshotEntry
  path: snapshot-relative string
  kind: "regular_file" | "directory" | "symlink" | "other"
  size_bytes: integer | null
  modified: TimestampValue
  metadata_status: "complete" | "partial" | "unsupported"

SnapshotFindMatch
  snapshot_id: string
  created_at_unix_seconds: integer
  tags: array of strings
  path: snapshot-relative string
  kind: "regular_file" | "directory" | "symlink" | "other"
  size_bytes: integer | null
  modified: TimestampValue
  metadata_status: "complete" | "partial" | "unsupported"
  match_reasons: array containing "path", "name", "glob", or "tag"

SnapshotDiffEntry
  path: snapshot-relative string
  status: "added" | "removed" | "changed" | "unchanged"
  from: SnapshotEntry | null
  to: SnapshotEntry | null
  content_changed: boolean
  metadata_changed: boolean
  metadata_changes: array containing available changed metadata field names,
    such as "kind", "size", "modified", "created", "symlink_target",
    "unix_mode", "unix_owner", "xattrs_status", "acls_status",
    "file_flags_status", "resource_forks_status",
    "windows_attributes_status", "sparse_extents_status", or
    "source_platform"

RepositoryFormatStatus
  format_version: integer
  latest_supported_format_version: integer
  compatibility: "current" | "unsupported_future" | "unsupported_legacy" |
                 "unsupported_features"
  features: array of strings

RepositoryStorageStatus
  conditional_create: boolean
  atomic_visibility: boolean
  strong_read_after_write: boolean
  delete: "unsupported" | "best_effort" | "idempotent"
  listing: "unsupported" | "prefix"
  repository_requirements_met: boolean

RepositoryObjectFamilyStatus
  family: "bootstrap" | "key_slot" | "key_slot_removal" | "commit" |
          "forget_marker" | "manifest" | "index" | "chunk" | "policy" |
          "upload_state" | "lease_state" | "prune_state" | "rekey_state" |
          "other"
  objects: integer
  status: "empty" | "present" | "verified"

RepositoryVerificationStatus
  unlocked: boolean
  key_slots: integer
  metadata_objects_checked: integer
  chunk_objects_checked: integer
  bytes_read: integer
  read_data_mode: "metadata_only"
  forget_markers_checked: integer
  policy_configs_checked: integer
  upload_states_checked: integer
  lease_states_checked: integer
  prune_plans_checked: integer
  prune_completions_checked: integer

TimestampValue
  status: "captured" | "unsupported" | "denied"
  seconds: integer, present only when status is "captured"
  nanoseconds: integer, present only when status is "captured"
  denial_reason: string, present only when status is "denied"

RestoreMetadataWarning
  entry_id: snapshot entry identifier; currently the snapshot-relative path
  path: snapshot-relative string
  namespace: metadata namespace, for example "portable", "unix", or the
             source platform namespace for platform-extension status warnings
  field: string
  source_platform: "windows" | "macos" | "linux" | "unix" | "unknown"
  destination_platform: same values as source_platform
  reason: string

CheckFinding
  code: stable string
  severity: "warning" | "error"
  object_key: string | null
  snapshot_id: string | null
  path: snapshot-relative string | null
  message: string

RetentionPolicySummary
  keep_last: integer | null
  keep_hourly: integer | null
  keep_daily: integer | null
  keep_weekly: integer | null
  keep_monthly: integer | null
  keep_yearly: integer | null
  keep_tags: array of strings

PolicyConfigItem
  policy_id: string
  policy_object: repository object key string
  created_at_unix_seconds: integer
  retention: RetentionPolicySummary

ForgetSnapshotItem
  snapshot_id: string
  created_at_unix_seconds: integer
  tags: array of strings
  action: "keep" | "forget"
  reasons: array of strings
  marker_object: string | null

PruneObject
  object_key: repository object key string
  kind: "bootstrap" | "key_slot" | "key_slot_removal" | "commit" | "forget_marker" | "manifest" | "index" | "chunk" | "policy" | "upload_state" | "lease_state" | "prune_state" | "other"
  bytes: integer | null

KdfSummary
  algorithm: "argon2id_v19"
  memory_cost_kib: integer
  time_cost: integer
  parallelism: integer
```

`completion <SHELL>` writes the requested shell script directly to stdout. It
does not support JSON wrapping because the completion script itself is the data.

## JSONL Event Envelope

`--jsonl` emits newline-delimited events:

```json
{"schema_version":1,"event":"command_started","command":"version","status":"started","data":null}
{"schema_version":1,"event":"command_completed","command":"version","status":"success","data":{"command":"ferry","version":"1.0.0"}}
```

Reserved event names:

```text
command_started
progress
warning
command_completed
command_failed
```

Long-running commands must emit at least `command_started` and either
`command_completed` or `command_failed`.
For successful commands, `command_started` is first and `command_completed` is
last. For failed commands after CLI parsing, `command_started` is first and
`command_failed` is last. Command-specific `progress` and `warning` events, if
present, are emitted between those boundary events.

Current schema:

```text
CommandEvent<T>
  schema_version: integer, currently 1
  event: "command_started" | "progress" | "warning" | "command_completed" | "command_failed"
  command: string
  status: "started" | "success" | "failure"
  data: T | null
```

Long-operation JSONL event data schemas:

```text
command_started
  data: null in the current implementation. Future schema versions may add
        request metadata here; v1 consumers must not require it.

progress
  data.phase: stable string
  data.message: string
  data.items_done: integer | null
  data.items_total: integer | null
  data.bytes_done: integer | null
  data.bytes_total: integer | null
  data.snapshot_id: string | null
  data.object_key: string | null

warning
  data: command-specific warning data. Restore currently emits
        RestoreMetadataWarning.

command_completed
  data: same command-specific data as the matching JSON document

command_failed
  data.code: stable string
  data.message: string
  data.exit_code: integer
  data.retryable: boolean
  data.path: redacted string | snapshot-relative string | null
  data.object_key: string | null
```

Required long-running commands must emit at least these phase names when the
phase applies:

```text
init: validate_repository, create_bootstrap, write_key_slot, complete
backup: walk_sources, plan_chunks, write_chunks, write_index, write_manifest, write_commit, complete
restore: load_manifest, read_chunks, write_entries, apply_metadata, verify, complete
check: load_commits, verify_metadata, verify_indexes, read_data, complete
doctor: inspect_repository, verify_metadata, verify_auxiliary_state,
        read_data, complete
forget: load_snapshots, evaluate_policy, write_forget_state, complete
prune: plan, mark, sweep, verify_reachability, complete
key add: load_bootstrap, derive_key, write_key_slot, complete
key remove: load_bootstrap, verify_remaining_unlock, remove_key_slot, complete
key rotate: load_bootstrap, derive_key, write_key_slot, retire_old_slots, complete
key rekey: load_bootstrap, derive_new_master_key, rewrite_objects,
           switch_bootstrap, cleanup_old_objects, complete
key export-recovery: load_bootstrap, create_export, complete
key import-recovery: load_bootstrap, read_export, write_key_slot, complete
```

Implemented command events:

```text
init command_started
  status: "started"
  data: null

init command_completed
  status: "success"
  data: Init data schema above

backup command_started
  status: "started"
  data: null

backup progress
  status: "started"
  data.phase: "walk_sources" | "plan_chunks" | "write_chunks" | "write_index" | "write_manifest" | "write_commit" | "complete"
  data.message: string
  data.items_done: integer | null
  data.items_total: integer | null
  data.bytes_done: integer | null
  data.bytes_total: integer | null
  data.snapshot_id: string | null
  data.object_key: string | null

backup command_completed
  status: "success"
  data: Backup data schema above

restore command_started
  status: "started"
  data: null

restore progress
  status: "started"
  data.phase: "load_manifest" | "read_chunks" | "write_entries" | "apply_metadata" | "verify" | "complete"
  data.message: string
  data.items_done: integer | null
  data.items_total: integer | null
  data.bytes_done: integer | null
  data.bytes_total: integer | null
  data.snapshot_id: string | null
  data.object_key: string | null

restore command_completed
  status: "success"
  data: Restore data schema above

snapshots command_started
  status: "started"
  data: null

snapshots command_completed
  status: "success"
  data: Snapshots data schema above

ls command_started
  status: "started"
  data: null

ls command_completed
  status: "success"
  data: Ls data schema above

find command_started
  status: "started"
  data: null

find command_completed
  status: "success"
  data: Find data schema above

diff command_started
  status: "started"
  data: null

diff command_completed
  status: "success"
  data: Diff data schema above

repo command_started
  status: "started"
  data: null

repo command_completed
  status: "success"
  data: Repo data schema above

doctor command_started
  status: "started"
  data: null

doctor progress
  status: "started"
  data.phase: "inspect_repository" | "verify_metadata" |
              "verify_auxiliary_state" | "read_data" | "complete"
  data.message: string
  data.items_done: integer | null
  data.items_total: integer | null
  data.bytes_done: integer | null
  data.bytes_total: integer | null
  data.snapshot_id: null
  data.object_key: null

doctor command_completed
  status: "success"
  data: Doctor data schema above

check command_started
  status: "started"
  data: null

check progress
  status: "started"
  data.phase: "load_commits" | "verify_metadata" | "verify_indexes" | "read_data" | "complete"
  data.message: string
  data.items_done: integer | null
  data.items_total: integer | null
  data.bytes_done: integer | null
  data.bytes_total: integer | null
  data.snapshot_id: string | null
  data.object_key: string | null

check command_completed
  status: "success"
  data: Check data schema above

check command_failed
  status: "failure"
  data: command_failed data schema above
  data.finding: CheckFinding, present when the check failure maps to a
    repository integrity finding

policy set command_started
  status: "started"
  data: null

policy set command_completed
  status: "success"
  data: PolicySet data schema above

policy show command_started
  status: "started"
  data: null

policy show command_completed
  status: "success"
  data: PolicyShow data schema above

policy delete command_started
  status: "started"
  data: null

policy delete command_completed
  status: "success"
  data: PolicyDelete data schema above

key add command_started
  status: "started"
  data: null

key add command_completed
  status: "success"
  data: KeyAdd data schema above

key rekey command_started
  status: "started"
  data: null

key rekey progress
  status: "started"
  data: ProgressData with the key rekey phase names above

key rekey command_completed
  status: "success"
  data: KeyRekey data schema above

version command_started
  status: "started"
  data: null

version command_completed
  status: "success"
  data.command: "ferry"
  data.version: semantic-version string from the package version
```

Future long-running commands must keep human progress off stdout in both JSON
and JSONL modes. Machine progress belongs in JSONL `progress` events.

## Current Commands

`ferry init` creates an encrypted local filesystem repository when `--repo` or
`FILEFERRY_REPOSITORY` points at a local path or `file:///absolute/path`.
It also creates encrypted S3-compatible repositories when the repository URL
has the form `s3://bucket[/prefix]`. The bucket and prefix come from the
repository URL; credentials must not be embedded in the URL. S3 init requires
`FILEFERRY_S3_ENDPOINT`, `FILEFERRY_S3_REGION`,
`FILEFERRY_S3_ACCESS_KEY_ID`, and `FILEFERRY_S3_SECRET_ACCESS_KEY` in the
environment. S3 endpoints must be HTTPS URLs by default. Local development
runtimes may use `http://localhost`, `http://127.*`, or `http://[::1]` only
when `FILEFERRY_S3_ALLOW_INSECURE_HTTP=1` is set explicitly. Prefix segments
currently use FileFerry object-key characters: ASCII letters, digits, `.`, `_`,
`-`, and `=`, separated by `/`. Set
`FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE=1` only for providers, such as the
current Backblaze B2 development path, that reject create-only `PutObject`
requests.
S3 endpoint values with embedded credentials, query strings, or fragments are
rejected before the object-store client is built.

Init requires `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE` for both local
and S3-compatible repositories. Human, JSON, and JSONL init output redacts S3
repository URLs as `s3://<redacted>` and does not emit S3 credentials.

Repository commands now resolve the repository backend through the same
local/S3 target parser before command execution. S3-compatible URLs with
embedded credentials, query strings, or fragments are rejected before use.
S3-compatible `init`, `backup`, `snapshots`, `ls`, `restore`, `check`,
`find`, `diff`, `repo`, `forget`, `prune`, `policy`, and key-management
commands use the explicit S3 environment contract above and redact repository
URLs as `s3://<redacted>`.

`ferry backup <SOURCE>...` opens an initialized local or S3-compatible
repository with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, creates an
encrypted, compressed, deduplicated snapshot through the core backup pipeline,
and commits it so `ferry snapshots` and `ferry ls` can discover it. `--tag
<TAG>` may be repeated. JSON output follows the Backup data schema above;
JSONL output emits the implemented progress phases listed above. Source paths
are local filesystem paths.

`ferry restore <DESTINATION>` opens an initialized local or S3-compatible
repository with `FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, selects
`latest` by default or accepts `--snapshot <ID>` / `--tag <TAG>` /
`--latest`, and restores directory entries, regular-file contents, and Unix
symlinks under the destination directory. `--path <PATH>` may be repeated to
restore snapshot-relative paths. If any requested snapshot-relative path
matches no manifest entry, the command fails with exit code `7` before
destination writes. The command fails if a destination file already exists unless
`--overwrite` is supplied. Destination safety for the selected directory,
regular-file, and symlink entries is preflighted before any destination writes
begin. Restored symlink destination paths and symlinked ancestors are rejected
if they already exist; symlinks are created after directory and regular-file
writes so restore writes do not traverse newly restored symlinks. A
path-scoped symlink restore creates missing parent directories under the
destination root after destination safety preflight. Non-dry-run restores also
reject selected manifest entries that would collide on a destination filesystem
observed to be case-insensitive. Windows destinations reject selected paths
with Windows reserved-name segments before destination writes. `--dry-run`
reports selected entries and planned writes without creating destination
entries. It also reports
`metadata_planned`, the count of regular-file and directory modified timestamp
and creation/birth timestamp fields, captured Unix permission and ownership
fields, selected symlink timestamp plus captured Unix symlink metadata fields,
selected reportable xattr status fields where xattrs were observed or xattr
capture was denied, and selected ACL status fields where ACLs were observed or
ACL capture was denied in constructed or future manifests, plus selected file
flag status fields where file flags were observed or file flag capture was
denied in constructed or future manifests, plus selected resource fork status
fields where resource forks were observed or resource fork capture was denied
in constructed or future manifests, plus selected Windows attribute status
fields where Windows attributes were observed or Windows attribute capture was
denied in constructed or future manifests, plus selected sparse extent status
fields where sparse extents were observed or sparse extent capture was denied
in constructed or future manifests. JSON output follows
the Restore data schema above; JSONL output emits the
implemented progress phases listed above. Current metadata application is
limited to captured modified timestamps for restored regular files and
directories, plus captured regular-file and directory Unix permission bits
(`0o777`) on Unix destinations. Restore also verifies captured regular-file
and directory Unix UID/GID ownership after writes and warns when destination
ownership does not match, but it does not call `chown`. Symlink targets are
restored, but symlink timestamps and captured Unix symlink mode/ownership are
reported as not restored. Creation/birth time for regular files and
directories is reported as a structured `portable`/`created` warning because
this version does not restore it. Linux currently has read-side birth-time
evidence through `statx(2)` when the filesystem reports `STATX_BTIME`, but
the normal Linux timestamp setters update only access and modification times;
macOS and Windows have creation-time APIs with filesystem or volume caveats,
but FileFerry has not yet implemented and tested those platform-specific
restore primitives. Reportable xattr presence/count status is
captured where xattr listing is exposed, but xattr names and values are not
restored; the observed macOS `com.apple.provenance` implementation detail is
not counted as reportable restore metadata. ACL status scaffolding is present
in manifests, but this version records ACL status as unsupported during normal
capture and does not read or restore ACL contents. File flag status
scaffolding is present in manifests, but this version records file flag status
as unsupported during normal capture and does not read or restore file flag
values. Windows attribute status scaffolding is present in manifests, but this
version records Windows attribute status as unsupported during normal capture
and does not read or restore Windows attribute values. Resource fork status
scaffolding is present in manifests, but this version records resource fork
status as unsupported during normal capture and does not read or restore
resource fork values. Unix ownership changes, Unix special mode bits, ACLs,
xattr values, resource forks, Windows attributes, file flags, sparse extents,
and other platform-specific metadata are not restored yet. Sparse extent
status scaffolding is present in manifests, but this version records sparse
extent status as unsupported during normal capture and does not read or
restore sparse extent maps. If a selected timestamp, Unix mode, Unix
ownership, creation/birth timestamp, symlink metadata field, xattr status
field, ACL status field, file flag status field, resource fork status field,
Windows attribute status field, or sparse extent status field cannot be
applied, represented, or restored by this version, or if dry-run planning
determines that the selected metadata is denied, unsupported, unrepresentable,
or outside the destination system time range, restore reports a
`metadata_warnings` item and exits with partial-success code `10`; JSON and
JSONL modes keep those warnings on stdout.

`ferry find` opens an initialized local or S3-compatible repository with
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, decrypts committed snapshot
manifests, and searches snapshot-relative entry metadata. It searches the
latest snapshot by default, or accepts repeated `--snapshot <ID>`, repeated
`--tag <TAG>`, `--latest`, or `--all` as mutually exclusive snapshot scopes.
Entry predicates are repeated `--path <PATH>`, repeated `--name <NAME>`, and
repeated `--glob <GLOB>`. `--path` matches the exact snapshot-relative path or
its subtree. `--name` matches the final path segment exactly. `--glob` matches
snapshot-relative paths with `*`, `?`, and `**` path segments. A tag-only find
lists non-root entries from snapshots carrying the requested tag. Find does
not search file contents and does not read chunk data. A find request with no
entry predicate and no tag scope fails as invalid input. A well-formed find
request with no matching entries fails with exit-code family `7`.

`ferry diff` opens an initialized local or S3-compatible repository with
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, decrypts committed snapshot
manifests, and compares two snapshot selections. Each side requires exactly
one selector: `--from-snapshot <ID>`, `--from-tag <TAG>`, or `--from-latest`
for the older side, and `--to-snapshot <ID>`, `--to-tag <TAG>`, or
`--to-latest` for the newer side. `--path <PATH>` may be repeated to compare
only exact snapshot-relative paths or subtrees. A path scope that exists in
neither snapshot fails with exit code `7`. Diff reports added, removed,
changed, and unchanged entry counts plus per-entry status. For regular files,
`content_changed` is derived from manifest chunk ids, offsets, and lengths.
Diff does not read chunk objects or compare file contents byte-by-byte.
`metadata_changes` is limited to metadata fields present in the decrypted
manifest; unsupported or not-yet-restored platform metadata is reported only
as status changes, not as value-level comparisons.

`ferry repo` inspects local or S3-compatible repository status. Without
`--verify`, it does not require a passphrase and reports only safe operational
facts: initialized/uninitialized status, redacted repository URL, backend
kind, format compatibility, repository id, passive storage capabilities, and
object-family aggregate counts. It must not print decrypted snapshot paths,
tags, source names, object keys, or per-snapshot/object shape in default
output. Unsupported future format versions and unsupported feature flags are
reported in the successful status document so operators can inspect a
repository before unlock. `ferry repo --verify` requires
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, unlocks the repository, and
authenticates committed manifests, chunk indexes, forget markers, policy
config objects, upload state, lease state, and prune state. It uses
metadata-only repository checking and does not read chunk data. The command
does not repair repositories.

`ferry doctor` diagnoses local or S3-compatible repository health without
repairing repository state. It reports safe operational facts and health
summaries by default: initialized/uninitialized status, redacted repository
URL, backend kind, format compatibility, storage requirement status, metadata
verification counts, auxiliary-state verification counts, and repair
availability. It does not print decrypted snapshot paths, tags, source names,
object keys, per-snapshot state, or object-family counts by default. For a
current initialized repository, doctor requires `FILEFERRY_PASSWORD` or
`FILEFERRY_PASSWORD_FILE`, unlocks the repository, authenticates committed
manifests, chunk indexes, forget markers, policy config objects, upload state,
lease state, and prune state, and reads no chunk data unless `--read-data` or
`--read-data-subset <N|PERCENT>` is supplied. `--show-object-counts` explicitly
adds aggregate object-family counts. Unsupported future format versions and
unsupported feature flags are reported as incompatible diagnostics without
unlock. Runtime doctor failures hide object keys and filesystem paths in the
default failure envelope; use lower-level commands such as `check` or
`repo --verify` when object-key context is required for manual repair.

`ferry check` opens an initialized local or S3-compatible repository with
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, authenticates committed
snapshot manifests, authenticates chunk indexes, reads every referenced chunk
object, decompresses chunk payloads, and verifies keyed chunk identities. JSON
output follows the Check data schema above with `read_data_mode: "full"` and
`read_data_subset: null` when no subset is requested. With
`--read-data-subset <N|PERCENT>`, check still authenticates all committed
metadata and validates manifest/index references, then reads a deterministic
subset of referenced chunk data selected from sorted chunk identities. Counts
must be positive integers. Percent values use `1%` through `100%`, select the
ceiling of the referenced chunk count, and select zero chunks only when the
repository has no referenced chunks. Subset JSON/JSONL output uses
`read_data_mode: "subset"` and reports the normalized requested value in
`read_data_subset`, such as `"5"` or `"5%"`. Invalid subset arguments fail with
exit code `2`. Check failures still fail closed. In JSON and JSONL modes,
runtime check failures emit a machine-readable failure envelope with a stable
`code`, `exit_code`, optional `object_key`, and, for repository integrity
failures, a `finding` object shaped like `CheckFinding`. Encrypted object
authentication failures retain the repository object key when the failing
object is known. Chunk-reference integrity failures retain the
snapshot-relative path, snapshot id, and object key when the committed manifest
provides that context. Decrypted manifests with invalid entry paths, duplicate
entry paths, non-file chunk references, regular-file size/chunk-length
mismatches, or non-directory ancestors fail as `snapshot_manifest_invalid`
integrity errors with snapshot id, object key, and path context when available.
Unsupported decrypted chunk-index schema versions fail as `chunk_index_invalid`
integrity errors with object-key context.

`ferry forget` opens an initialized local or S3-compatible repository with
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, authenticates currently
visible committed snapshot manifests, evaluates retention keep rules, and
writes immutable snapshot forget markers for snapshots not retained by those
rules. It never deletes chunks, manifests, indexes, or commit objects; object
deletion is handled only by the separate `ferry prune` command.
Non-dry-run forget first writes an encrypted best-effort command lease under
`locks/`, rejects another active readable lease with exit code `3` and
`repository_locked`, ignores expired readable leases, and fails closed on
malformed or unauthenticatable lease objects before writing markers. After
marker writes return, forget best-effort deletes its own lease; if that release
is interrupted, the lease expires by timestamp. `--dry-run` evaluates the same
plan but writes no forget markers or lease state. The
implemented keep rules are `--keep-last <N>`, `--keep-hourly <N>`,
`--keep-daily <N>`,
`--keep-weekly <N>`, `--keep-monthly <N>`, `--keep-yearly <N>`, and repeatable
`--keep-tag <TAG>`. Count values must be greater than zero. Alternatively,
`--policy <POLICY_ID>` authenticates and applies exactly one encrypted
repository-local policy config by id after repository unlock. Inline retention
flags and `--policy` are mutually exclusive; mixed selection fails with exit
code `2` and `forget_policy_selection_invalid`. Forget never chooses a stored
policy implicitly, including when one or more policy config objects exist.
Missing policy ids fail with exit code `7` and
`repository_policy_config_not_found`; malformed, tampered, or invalid selected
policy objects fail closed as repository integrity failures. A policy that
would forget no currently visible snapshots fails with exit code `7` and
`forget_no_snapshots_matched`; invalid or empty inline policies fail with exit
code `2`. JSON output reports policy source and id, candidate, kept, and
forgotten snapshot items, item-level reasons, dry-run status, and marker
objects written. JSONL output emits `load_snapshots`, `evaluate_policy`,
`write_forget_state`, and `complete` progress phases.

`ferry policy set` opens an initialized local or S3-compatible repository with
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE` and writes one encrypted
repository-local retention policy config object under `objects/policy/`.
Supported rules match the retention rule flags accepted by `forget`:
`--keep-last`, `--keep-hourly`, `--keep-daily`, `--keep-weekly`,
`--keep-monthly`, `--keep-yearly`, and repeated `--keep-tag`. Count values
must be greater than zero, and at least one keep rule is required. Repeating
the same retention body through the CLI is idempotent and reports the existing
policy object instead of writing another object. Different retention bodies can
coexist as separate encrypted policy configs. `ferry policy show` unlocks the
repository and displays the stored policy configs; if none exist, it fails with
exit code `7` and `repository_policy_config_not_found`. `ferry policy delete
<POLICY_ID>` authenticates the selected policy config before deletion and
supports `--dry-run`; missing policy ids fail with exit code `7`. Stored
policies are applied to forget only through explicit
`ferry forget --policy <POLICY_ID>` selection and are not chosen implicitly.

`ferry prune` opens an initialized local or S3-compatible repository with
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE` and deletes only objects
that are reachable from forgotten committed snapshots and not reachable from
any non-forgotten committed snapshot. It never deletes `bootstrap`, key-slot
objects, key-slot-removal markers, policy objects, upload-state objects,
lease-state objects, unknown objects, or prune state. `--dry-run` uses the
same reachability logic but writes no prune state or lease state and deletes
nothing. A real prune first writes an encrypted best-effort command lease under
`locks/`, rejects another active readable lease with exit code `3` and
`repository_locked`, ignores expired readable leases, and fails closed on
malformed or unauthenticatable lease objects. After the sweep returns, prune
best-effort deletes its own lease; if that release is interrupted, the lease
expires by timestamp. A real prune writes an
encrypted durable prune plan under `objects/prune-plan/` before deleting
candidates, then writes an encrypted completion object under
`objects/prune-completion/` after the sweep. If a previous plan has no
completion object, the next real prune resumes it. If a completion object
exists for a marked plan, prune authenticates and validates that completion
state before treating the plan as complete. Missing candidate objects during
sweep are reported as already gone, not as corruption. Before and during
sweep, prune compares the current commit/forget marker state against the
marked plan, allowing for objects already deleted by that same plan; a new
commit or forget marker aborts the sweep with exit code `8` and
`repository_prune_state_changed`. Malformed prune plan or completion state
fails closed as an integrity failure with exit code `6`. JSON and JSONL output
report candidate, retained, deleted, and missing objects plus byte counts
where object bytes were available. JSONL output emits `plan`, `mark`,
`verify_reachability`, `sweep`, and `complete` progress phases.
S3-compatible prune uses the same encrypted object-store prune pipeline and
does not require rename operations for correctness; provider-specific
lifecycle policies and arbitrary S3 repair remain out of scope.

`ferry snapshots` opens an initialized local or S3-compatible repository with
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, authenticates committed
snapshot manifests that have not been marked forgotten, and emits human, JSON,
or JSONL-safe snapshot summaries.
If a committed snapshot marker references a missing manifest object, the
command fails closed with an integrity failure instead of treating the
repository as uninitialized.

`ferry ls` opens an initialized local or S3-compatible repository, selects
`latest` by default or accepts `--snapshot <ID>` / `--tag <TAG>`, and lists
immediate entries at a snapshot-relative path. JSON output uses `"."` for the
snapshot root path.

`ferry key add` opens an initialized local or S3-compatible repository with
the existing passphrase from `FILEFERRY_PASSWORD` or
`FILEFERRY_PASSWORD_FILE`, then writes one immutable additional passphrase
key-slot object. The new passphrase must come from `--new-password-file`,
`FILEFERRY_NEW_PASSWORD`, or
`FILEFERRY_NEW_PASSWORD_FILE`; the command does not prompt interactively.
Output includes the repository id, new key-slot id, total visible key-slot
count, KDF parameters, and `reencrypted_repository_objects: false`. The
command does not create a new master key, rewrite the original bootstrap slot,
rewrite encrypted chunks, manifests, indexes, commit markers, forget markers,
or policy/config objects, and does not recover a lost master key. Wrong
existing passphrases fail with exit code `4`; malformed key-slot or bootstrap
state fails closed as an integrity failure with exit code `6`.

`ferry key remove <KEY_SLOT_ID>` opens an initialized local or S3-compatible
repository with the current passphrase from `FILEFERRY_PASSWORD` or
`FILEFERRY_PASSWORD_FILE`, then writes one immutable
`key-slot-removals/<key-slot-id>` marker for an externally added key slot. It
does not delete `key-slots/<key-slot-id>` objects, remove the original
bootstrap key slot, create a new master key, rewrite encrypted repository
objects, or recover lost keys. The supplied current passphrase must prove a
remaining non-removed unlock path before a marker is written; otherwise the
command fails closed with exit code `4`. Missing key-slot ids fail with exit
code `7`; malformed key-slot or key-slot removal marker state fails as an
integrity failure with exit code `6`. Output includes the repository id,
removed key-slot id, visible key-slot count, removal marker object,
`removal_marker_created`, `deleted_key_slot_objects: false`, and
`reencrypted_repository_objects: false`.

`ferry key rotate --retire-key-slot <KEY_SLOT_ID>...` opens an initialized
local or S3-compatible repository with the current passphrase from
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, reads the new passphrase from
`--new-password-file`, `FILEFERRY_NEW_PASSWORD`, or
`FILEFERRY_NEW_PASSWORD_FILE`, writes one immutable new key-slot object for
the existing repository master key, proves that new slot unlocks the master
key, then writes immutable `key-slot-removals/<key-slot-id>` markers for only
the explicitly selected externally added slots. It does not delete key-slot
objects, remove the original bootstrap key slot, remove unselected key slots,
create a new master key, rewrite encrypted repository objects, or recover
lost keys. Missing selected key-slot ids fail with exit code `7`; wrong
current passphrases fail with exit code `4`; malformed key-slot or removal
marker state fails as an integrity failure with exit code `6`. Output
includes the repository id, added key-slot id, removed key-slot ids, visible
key-slot count, removal marker objects, removal marker creation count, KDF
parameters, `deleted_key_slot_objects: false`, and
`reencrypted_repository_objects: false`.

`ferry key rekey` opens an initialized local or S3-compatible repository with
the current passphrase from `FILEFERRY_PASSWORD` or
`FILEFERRY_PASSWORD_FILE`, reads the replacement passphrase from
`--new-password-file`, `FILEFERRY_NEW_PASSWORD`, or
`FILEFERRY_NEW_PASSWORD_FILE`, creates a new repository master key, rewrites
encrypted chunks, indexes, manifests, policy configs, upload state, prune
state, and snapshot visibility markers for that new master key, then switches
`bootstrap` to a single new embedded key slot. Existing external key slots are
retired with removal markers and their old `key-slots/<key-slot-id>` objects
are deleted during cleanup. Old passphrases and old external key slots do not
unlock the completed repository unless the operator deliberately reuses the
same passphrase value for the new bootstrap slot. A pending
`objects/rekey/<prefix>/<rekey-id>` state is encrypted for both old and new
masters so the command can resume after interruption; cleanup after the
bootstrap switch can be resumed with the new passphrase. Malformed, tampered,
stale, or replayed state fails closed. Active readable leases reject the
command before object rewrites. Output includes rewrite counts,
`old_unlocks_retained: false`, `raw_master_key_exported: false`, and
`reencrypted_repository_objects: true`.

`ferry key export-recovery --output <FILE>` opens an initialized local or
S3-compatible repository with the current passphrase from
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, creates one standalone
encrypted recovery export file, and writes it only if the destination file
does not already exist. The export is encrypted and authenticated with
Argon2id v1.3 and
XChaCha20-Poly1305 using associated data documented in `docs/security.md`.
The current implementation protects the export with the current repository
passphrase and does not export raw master-key material, create a new master
key, rewrite repository objects, or re-encrypt repository objects. Wrong
passphrases fail with exit code `4`; an existing destination fails with exit
code `2`; malformed repository key state fails as an integrity failure with
exit code `6`. Output includes the repository id, export id, redacted
destination, visible key-slot count, creation time, KDF parameters, AEAD
algorithm, warning text, `recovery_import_implemented: true`,
`raw_master_key_exported: false`, and `reencrypted_repository_objects: false`.

`ferry key import-recovery --input <FILE>` opens an initialized local or
S3-compatible repository, reads an encrypted recovery export from the input
file, opens the target repository with the same passphrase from
`FILEFERRY_PASSWORD` or `FILEFERRY_PASSWORD_FILE`, decrypts the package with
that passphrase, and writes one new external key slot for the new passphrase
from `--new-password-file`, `FILEFERRY_NEW_PASSWORD`, or
`FILEFERRY_NEW_PASSWORD_FILE`. The package repository id must match the target
repository bootstrap, and the package master-key check must verify against the
opened repository master key before any key-slot write. Wrong
repository/package passphrases fail with exit code `4`; malformed, tampered,
unsupported, or repository-mismatched recovery packages fail closed as
integrity failures with exit code `6`; unreadable input files fail with exit
code `5`. Output includes the repository id, export id, redacted input source,
added key-slot id, visible key-slot count, package creation time, KDF
parameters, AEAD algorithm, `raw_master_key_exported: false`, and
`reencrypted_repository_objects: false`.

## Local Backend Failure Evidence

For initialized local repositories, current tests exercise these failure
families through command or core/storage boundaries:

- Missing objects referenced by committed repository metadata map to integrity
  failure exit code `6`.
- Malformed committed objects and authenticated-object failures map to
  integrity failure exit code `6`.
- Malformed encrypted policy/config objects, invalid policy/config metadata,
  and policy/config identity mismatches map to integrity failure exit code `6`
  with `repository_policy_config_decode_failed`,
  `repository_policy_config_invalid`, or
  `repository_metadata_identity_mismatch` when surfaced through a CLI path.
- Immutable write conflicts map to storage/filesystem failure exit code `5`.
- Permission-denied source reads during backup map to filesystem I/O failure
  exit code `5` when the platform exposes the denial to the test process.
- Stale `.fileferry-tmp/*.part` files and malformed uncommitted objects are
  not treated as committed snapshots. They are not cleaned up automatically.

JSON and JSONL failure envelopes preserve safe repository object keys for
object-scoped failures when the core or storage layer knows the key. Path
context is redacted before it is emitted. This is local-backend evidence only;
it is not a repair, prune, or S3-compatible backend support claim.

`ferry version` supports human, JSON, and JSONL output.

`ferry completion <SHELL>` writes shell completion data for Bash, Elvish,
Fish, PowerShell, and Zsh.
