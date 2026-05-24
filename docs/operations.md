# Operations

Operational notes for behavior that has been exercised through the `ferry`
binary. Keep this document evidence-led: record only drills that were actually
run, and keep backend scope explicit.

## Local Restore Release Drill - 2026-05-22

Scope:

- Backend: local filesystem repository in a temporary directory.
- Evidence command:
  `cargo test -p fileferry-cli --test local_restore_drills`.
- Commands exercised through the `ferry` binary: `init`, two `backup`
  snapshots, full `restore`, path-scoped `restore`, `restore --latest`, and
  full `check`.
- Snapshot selection: `--tag release-drill`, explicit `--snapshot`, and
  `--latest`.
- Restored entry kinds: directories, regular files, and Unix symlinks on Unix
  hosts.
- Verification: restored file bytes were compared against the original source
  bytes, a restored regular-file modified timestamp was compared against the
  source timestamp, the empty nested directory was verified, the Unix symlink
  target was verified on Unix hosts, path-scoped restore was checked not to
  write unselected files, `--latest` was checked to select the newest snapshot,
  and `ferry check` authenticated committed metadata and chunk objects.

Observed result on this host:

- Test result: passed.
- Full restore wrote and verified two regular files.
- Full restore wrote the empty nested directory tree.
- Full restore wrote one Unix symlink on this Unix host.
- Path-scoped restore selected and verified one regular file without writing
  the unselected blob.
- Latest restore selected the second committed snapshot.
- Full repository check completed with no check errors or warnings.

This is local-backend release evidence only. It does not claim
S3-compatible restore coverage, release artifact coverage, or platform support
outside the host that ran the drill.

## Local Restore And Check Drill - 2026-05-18

Scope:

- Backend: local filesystem repository in a temporary directory.
- Commands: `ferry init`, `ferry backup`, `ferry restore`, `ferry check`.
- Snapshot selection: `--tag drill`.
- Restore scope: full snapshot.
- Restored entry kinds: directory entries, one regular file, and one Unix
  symlink.
- Verification: `cmp` compared source and restored file bytes, and restore JSON
  reported `verified_files: 1`; `test -d` verified the restored empty nested
  directory; `readlink` verified the restored symlink target; `ferry check`
  authenticated the committed manifest, chunk index, and referenced chunk.
  `stat` compared source and restored modified timestamps for the regular file
  and the nested directory. The command transcript below uses the macOS
  `stat -f %m` form that was run locally.

Result:

- Snapshot id:
  `2ca38d8e22e8cf7ac786e3f8c4f25b471d7fb6423a1cbc58c7e78846b12361f5`
- Entries selected: `5`.
- Directories written: `3`.
- Files written: `1`.
- Symlinks written: `1`.
- Metadata planned: `4`.
- Metadata applied: `4`.
- Metadata warnings: `0`.
- Bytes written: `20`.
- Verified files: `1`.
- Check metadata objects: `3`.
- Check chunk objects: `1`.
- Check read data mode: `full`.
- Byte comparison: passed.
- Directory verification: passed.
- Symlink target verification: passed.

Command shape used:

```sh
root="$(mktemp -d)"
repo="$root/repo"
source="$root/source"
restore="$root/restore"
mkdir -p "$source/empty/nested"
printf 'restore drill bytes\n' > "$source/sample.txt"
ln -s sample.txt "$source/sample.link"
touch -mt 202311142213.20 "$source/sample.txt" "$source/empty" "$source/empty/nested" "$source"

FILEFERRY_PASSWORD='throwaway-passphrase' ferry --repo "$repo" init
FILEFERRY_PASSWORD='throwaway-passphrase' ferry --repo "$repo" --json backup --tag drill "$source"
FILEFERRY_PASSWORD='throwaway-passphrase' ferry --repo "$repo" --json restore --tag drill "$restore"
FILEFERRY_PASSWORD='throwaway-passphrase' ferry --repo "$repo" --json check
cmp "$source/sample.txt" "$restore/sample.txt"
test -d "$restore/empty/nested"
test "$(readlink "$restore/sample.link")" = 'sample.txt'
test "$(stat -f %m "$source/sample.txt")" = "$(stat -f %m "$restore/sample.txt")"
test "$(stat -f %m "$source/empty/nested")" = "$(stat -f %m "$restore/empty/nested")"
```

This drill does not claim S3-compatible restore coverage, metadata beyond
regular-file and directory modified timestamps, configurable check subset
coverage, or symlink restore behavior on non-Unix platforms.

## Local Backend Interruption And Corruption Evidence - 2026-05-19

Scope:

- Backend: local filesystem repository in temporary directories.
- Commands and boundaries: `ferry backup`, `ferry snapshots`, `ferry check`,
  and core/storage tests beneath the same local object-store model.
- Interruption simulation: stale `.fileferry-tmp/*.part` files and malformed
  uncommitted repository objects were added to initialized local repositories
  without corresponding commit markers.
- Corruption simulation: committed metadata and referenced chunk objects were
  removed, malformed, or tampered in tests.

Observed behavior:

- Stale local temporary objects are not returned by repository object listing.
  They are left in place; no cleanup or repair command is implemented yet.
- Malformed objects that are not referenced by a commit marker are ignored by
  `ferry snapshots` and `ferry check`.
- Missing objects referenced by committed metadata fail closed as integrity
  failures with exit code `6`, and JSON/JSONL failure envelopes include the
  safe repository `object_key` when it is known.
- Malformed committed objects and authenticated-object failures fail closed as
  integrity failures with exit code `6`. `ferry check` JSON/JSONL failures
  include a `finding` object when the core error carries enough context.
- Local immutable write conflicts are tested in the storage layer and through
  the core repository bootstrap boundary. They map to the storage/filesystem
  failure family, exit code `5`, at the CLI boundary.
- Unix unreadable source-file backup failures are tested through the CLI when
  the test process cannot read the file after permissions are removed. They
  map to the filesystem I/O failure family, exit code `5`, and JSON output
  preserves a redacted path without an object key.

Evidence added or retained:

- `fileferry-storage` tests: local put/get/list/delete, idempotent immutable
  writes, conflicting immutable writes with temporary-object cleanup, stale
  temporary object listing behavior, and the repository capability probe for
  required idempotent-delete/prefix-listing capabilities plus probe-object
  cleanup.
- `fileferry-core` tests: missing or tampered chunks, malformed or replayed
  metadata, malformed commits, manifest/index mismatches, invalid manifests,
  permission-denied source reads, immutable bootstrap write conflicts, local
  prune dry-runs, successful prune sweeps, interrupted prune resume, missing
  prune candidates, retained stale/unknown listed objects, prune delete
  permission failures, malformed prune state, and commit/forget state-change
  guardrails, plus restore destination guardrails for symlinked ancestors,
  observed case-insensitive path collisions, and Windows reserved names.
- `fileferry-storage` policy tests: retryable put and delete failures,
  permanent immutable-write conflicts, timeouts, and concurrency limiting.
- `fileferry-cli` tests: missing referenced manifests/chunks, tampered
  chunks, malformed commits, corrupted metadata, stale local temp/uncommitted
  partial objects, local prune JSON/JSONL output, malformed prune state exit
  mapping, JSON permission failure envelopes, S3 data-path missing-environment
  failure ordering, a gated live S3 data-path drill for init, backup,
  snapshots, ls, restore, check, and missing referenced manifests, and a
  gated live S3 retention/key-management drill for forget, key add, key
  remove, key rotate, key export-recovery, and key import-recovery, plus a
  gated live S3 prune drill for dry-run, sweep, durable prune state, snapshots,
  and unique-prefix cleanup. CLI unit coverage maps restore destination
  reserved-name and
  case-collision guardrails to stable JSON failure codes.

The non-gated evidence uses local and in-memory object stores. Current
Backblaze B2 S3-compatible provider evidence was observed on 2026-05-22 under
an isolated private development prefix with
`FILEFERRY_S3_INTEGRATION=1`, `FILEFERRY_S3_INIT_INTEGRATION=1`,
`FILEFERRY_S3_DATA_INTEGRATION=1`,
`FILEFERRY_S3_RETENTION_KEY_INTEGRATION=1`, and
`FILEFERRY_S3_PRUNE_INTEGRATION=1`. The passing live gates covered storage
capability probe, storage round-trip, CLI init, backup, snapshots, ls, restore,
check, missing referenced manifest failure, forget,
key add/remove/rotate/export-recovery, prune dry-run, prune sweep, durable
prune state, snapshots, and unique-prefix cleanup. This does not claim
automatic repair, cleanup of stale temporary files, provider-specific S3
lifecycle management, platform-wide permission behavior, release support on
every target, or support for every S3-compatible provider.
