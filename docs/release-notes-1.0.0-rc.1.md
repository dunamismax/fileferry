# FileFerry 1.0.0-rc.1

FileFerry `1.0.0-rc.1` is a release candidate for the first FileFerry CLI
release. It is not final v1.0.0.

FileFerry is an all-Rust encrypted backup CLI. The command is `ferry`.

## What Is Included

- Encrypted local and S3-compatible repository initialization.
- Encrypted, compressed, deduplicated snapshot creation.
- Restore by latest snapshot, snapshot id, tag, and path.
- `snapshots`, `ls`, `check`, `forget`, and recoverable two-phase `prune`.
- Key add, marker-based key remove, limited unlock rotation, and encrypted
  recovery export.
- JSON and JSONL machine output for the implemented command surface.
- Config profiles and environment-variable precedence.
- Shell completion generation.
- Format v0 repository compatibility fixtures for the documented current
  object families.

## Artifact Targets

Release-candidate artifacts are attached for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

For each target, the GitHub release includes the target archive, a target-named
`SHA256SUMS` file, a target-named `SHA256SUMS.sigstore.json` file, a CycloneDX
SBOM, a release manifest, and archive-smoke JSON evidence. Shared `install.sh`
and `install.ps1` installer scripts are also attached.

## Install

Download the archive and supporting files for your target from this GitHub
release.

Unix shell installer:

```sh
sh ./install.sh \
  --archive ./fileferry-1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz \
  --install-dir "$HOME/.local/bin"
```

PowerShell installer:

```powershell
pwsh -NoLogo -NoProfile -NonInteractive -File ./install.ps1 `
  -Archive ./fileferry-1.0.0-rc.1-x86_64-pc-windows-msvc.tar.gz `
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

Format v0 is frozen for the currently documented object families and
fixture-covered JSON shapes. Future format changes require an explicit version
or documented feature gate with fixtures.

This release candidate has security-sensitive tests for wrong-password,
wrong-key, tamper, corruption, malformed metadata, and secret redaction paths.
It is not an external security audit claim.

## Known Limitations

- This is not final v1.0.0.
- Restore applies only the implemented metadata subset.
- xattr values, ACL contents, file flags, resource forks, Windows attributes,
  sparse extent maps, symlink metadata, and creation/birth timestamps are not
  restored by this version.
- Key rotation rotates unlock access. It does not rewrite repository data with
  a new master key.
- Recovery export exists; recovery import and full repository rekey are not
  implemented.
- S3-compatible behavior is tested to the level documented in the repository.
  It is not a blanket claim for every S3-compatible provider.
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

Each release manifest records version `1.0.0-rc.1` and the commit used for the
tagged GitHub release.
