# FileFerry 1.0.0

FileFerry `1.0.0` is the first official FileFerry v1 release.

FileFerry is an all-Rust encrypted backup CLI. The command is `ferry`.

## Upgrade Impact

Repositories created by `1.0.0-rc.1` use the same documented format v0 object
families as `1.0.0`. The final release adds command surface and operational
evidence after the RC, but it does not introduce a repository-format migration.

Compared with `1.0.0-rc.1`, the final v1 release includes recovery import,
`ferry find`, `ferry diff`, `ferry repo`, `ferry policy`, `ferry doctor`, full
repository rekey, explicit stored-policy application through
`ferry forget --policy`, expanded S3-compatible command drills, and local
MinIO runtime evidence for the conditional-create path.

## What Is Included

- Encrypted local and S3-compatible repository initialization.
- Encrypted, compressed, deduplicated snapshot creation.
- Restore by latest snapshot, snapshot id, tag, and path.
- `snapshots`, `ls`, `find`, `diff`, `repo`, `doctor`, `check`, `forget`,
  `prune`, `policy`, and key-management commands.
- Key add, marker-based key remove, unlock rotation, full repository rekey,
  encrypted recovery export, and recovery import as a new external key slot.
- JSON and JSONL machine output for the implemented command surface.
- Config profiles and environment-variable precedence.
- Shell completion generation.
- Format v0 repository compatibility fixtures for the documented current
  object families.

## Artifact Targets

Release artifacts are attached for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

For each target, the GitHub release includes:

- `fileferry-1.0.0-<target>.tar.gz`
- `fileferry-1.0.0-<target>.SHA256SUMS`
- `fileferry-1.0.0-<target>.SHA256SUMS.sigstore.json`
- `fileferry-1.0.0-<target>.manifest.json`
- `fileferry-1.0.0-<target>.cdx.json`
- `fileferry-<target>.archive-smoke.json`

Shared `install.sh` and `install.ps1` installer scripts are also attached.

## Install

Download the archive and supporting files for your target from this GitHub
release.

Unix shell installer:

```sh
sh ./install.sh \
  --archive ./fileferry-1.0.0-x86_64-unknown-linux-gnu.tar.gz \
  --install-dir "$HOME/.local/bin"
```

PowerShell installer:

```powershell
pwsh -NoLogo -NoProfile -NonInteractive -File ./install.ps1 `
  -Archive ./fileferry-1.0.0-x86_64-pc-windows-msvc.tar.gz `
  -InstallDir "$HOME/bin"
```

Both installers verify the archive against `SHA256SUMS` when it is present
beside the archive. When using the target-named checksum file from the GitHub
release, pass it explicitly as the checksum file or save it as `SHA256SUMS`
beside the archive.

## Security And Format

Repositories are encrypted client-side before data is written to storage.
Authenticated encrypted objects cover file contents, file names, directory
structure, snapshot metadata, indexes, and sensitive repository config.

Format v0 is frozen for the documented object families and fixture-covered JSON
shapes. Future format changes require an explicit version or documented feature
gate with fixtures.

The release has security-sensitive tests for wrong-password, wrong-key, tamper,
corruption, malformed metadata, replay, and secret redaction paths. This is not
an external security audit claim.

## Known Limitations

- Restore applies only the implemented metadata subset.
- xattr values, ACL contents, file flags, resource forks, Windows attributes,
  sparse extent maps, symlink metadata, and creation/birth timestamps are not
  restored by this version.
- `ferry key rotate` rotates unlock access. Use `ferry key rekey` for a new
  repository master key and object rewrite.
- `ferry find` searches decrypted snapshot metadata after repository unlock. It
  does not search file contents or read chunk data.
- `ferry diff` compares decrypted snapshot manifests after repository unlock.
  It does not read chunk data or compare file contents byte-by-byte.
- `ferry repo` and `ferry doctor` are inspection and diagnostics paths. They do
  not repair repositories.
- Backblaze B2 is the only current cloud-provider evidence. Local MinIO is
  local S3-compatible runtime evidence. This is not a blanket claim for every
  S3-compatible provider.
- FileFerry remains CLI-only. There is no GUI, TUI, daemon, scheduler, server,
  SaaS dashboard, mobile app, FUSE mount, or compatibility mode for existing
  backup repository formats.

## Verification

The release artifacts are built by the manual GitHub release-artifacts workflow
with signing enabled. Each intended target artifact directory is verified with:

```sh
cargo run -p xtask -- verify-release-artifacts \
  --dir <artifact-dir> \
  --target <target> \
  --expect-signature
```

Each release manifest records version `1.0.0` and the commit used for the
tagged GitHub release.
