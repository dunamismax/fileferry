# Operations

Operational notes for behavior that has been exercised through the `ferry`
binary. Keep this document evidence-led: record only drills that were actually
run, and keep backend scope explicit.

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
  and the nested directory. The command transcript below uses the macOS/BSD
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
  writes, conflicting immutable writes with temporary-object cleanup, and
  stale temporary object listing behavior.
- `fileferry-core` tests: missing or tampered chunks, malformed or replayed
  metadata, malformed commits, manifest/index mismatches, invalid manifests,
  permission-denied source reads, and immutable bootstrap write conflicts.
- `fileferry-cli` tests: missing referenced manifests/chunks, tampered
  chunks, malformed commits, corrupted metadata, stale local temp/uncommitted
  partial objects, and JSON permission failure envelopes.

This evidence is local-backend evidence only. It does not claim automatic
repair, cleanup of stale temporary files, prune safety, S3-compatible backup,
S3-compatible restore, S3-compatible check, platform-wide permission behavior,
or release support on every target.
