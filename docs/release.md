# Release

This document defines the retained FileFerry release process until a dedicated
release tool such as `cargo-dist` is adopted.

`1.0.0-rc.1` is a release candidate. Do not publish or describe it as final
v1.0.0.

The release process must not claim platform support before CI, relevant tests,
release artifacts, checksums/signatures, SBOMs, cargo-auditable metadata, and
archive-smoke evidence exist for the exact release commit.

## Release-Candidate Targets

The intended `1.0.0-rc.1` target matrix is:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

Each target release directory must contain:

- `fileferry-1.0.0-rc.1-<target>.tar.gz`
- `SHA256SUMS`
- `SHA256SUMS.sigstore.json`
- `fileferry-1.0.0-rc.1-<target>.manifest.json`
- `fileferry-1.0.0-rc.1-<target>.cdx.json`
- `fileferry-<target>.archive-smoke.json`
- `install.sh`
- `install.ps1`

Every release manifest must record package `fileferry`, binary `ferry`,
version `1.0.0-rc.1`, the target triple, the exact release commit, an
auditable build, cargo-auditable metadata, the archive name, installer names,
and the target SBOM.

## CI Evidence

The normal CI workflow runs formatting, clippy, tests, and build on these
hosted runner targets:

- Ubuntu Linux x86_64 GNU host.
- Ubuntu Linux ARM64 GNU host.
- macOS Intel host.
- macOS ARM64 host.
- Windows x86_64 MSVC host.

Completed passing jobs are required release-candidate evidence, but they are
not platform support by themselves. Support language still needs the matching
target-specific release artifact, checksum/signature/SBOM/auditable metadata,
archive-smoke evidence, and relevant platform metadata tests for the exact
release candidate.

## Preconditions

Before tagging a release candidate:

- The checkout is clean and on `main`.
- `git pull --ff-only origin main` has completed.
- No secrets, `.env` files, private repositories, production logs, recovery
  exports, real backup repositories, or cloud bucket dumps are present in the
  tree.
- The configured Git identity belongs to `dunamismax`.
- The exact commit has passed the local gate:

```sh
just fmt
just check
env -u FILEFERRY_S3_INTEGRATION \
  -u FILEFERRY_S3_INIT_INTEGRATION \
  -u FILEFERRY_S3_DATA_INTEGRATION \
  -u FILEFERRY_S3_RETENTION_KEY_INTEGRATION \
  -u FILEFERRY_S3_PRUNE_INTEGRATION \
  just test
just build
git diff --check
```

Do not run live S3 tests for release publication unless the changes affect S3
behavior or Stephen explicitly asks for live provider evidence.

## Local Artifact Task

The retained local packaging entrypoint is:

```sh
cargo run -p xtask -- release-package --auditable --sbom
```

By default this builds the host `ferry` binary, packages the binary,
`README.md`, and `LICENSE`, copies the Unix shell and PowerShell installer
scripts beside the archive, writes `SHA256SUMS`, writes a release manifest,
generates a CycloneDX SBOM for the `ferry` binary, and smoke-tests the host
binary with `ferry version --json`.

Useful options:

```text
--target <TRIPLE>   Package a specific Rust target triple
--out-dir <DIR>     Write artifacts somewhere other than target/release-artifacts
--auditable         Build with cargo-auditable metadata
--sbom              Generate a CycloneDX JSON SBOM with cargo-cyclonedx
--sign              Sign SHA256SUMS with cosign sign-blob
--skip-smoke        Skip host binary and archive smoke tests
```

`--sign` requires a configured `cosign` identity or key. Local unsigned
artifacts are useful for dry runs, but they are not release artifacts.

The retained archive smoke entrypoint is:

```sh
version="1.0.0-rc.1"
host="$(rustc -vV | awk '/host:/ {print $2}')"
cargo run -p xtask -- archive-smoke \
  --archive "target/release-artifacts/fileferry-${version}-${host}.tar.gz" \
  --target "${host}" \
  --installers-dir target/release-artifacts \
  --expect-auditable \
  --out target/release-artifacts/fileferry-${host}.archive-smoke.json
```

`archive-smoke` verifies the archive against `SHA256SUMS` when it is present
beside the archive, extracts the archive, runs the packaged binary with
`ferry version --json`, verifies cargo-auditable metadata on the packaged
binary when `--expect-auditable` is supplied, and optionally runs available
installer scripts from `--installers-dir`.

The retained artifact-directory verifier is:

```sh
cargo run -p xtask -- verify-release-artifacts \
  --dir target/release-artifacts \
  --target "$(rustc -vV | awk '/host:/ {print $2}')" \
  --expect-signature
```

It parses the release manifest, CycloneDX SBOM, Sigstore bundle, and
archive-smoke evidence JSON, verifies target ownership, and checks
`SHA256SUMS` entries for the archive, manifest, SBOM, and installer scripts.

## Manual Workflow

The workflow `.github/workflows/release-artifacts.yml` is manual-only. It
builds candidate artifacts with `cargo-auditable`, generates SBOMs with
`cargo-cyclonedx`, and signs checksum manifests with Sigstore keyless signing
through GitHub OIDC when `sign_artifacts=true`.

The current native hosted matrix is:

- Linux x86_64 GNU on `ubuntu-latest`.
- Linux ARM64 GNU on `ubuntu-24.04-arm`.
- macOS x86_64 on `macos-15-intel`.
- macOS ARM64 on `macos-15`.
- Windows x86_64 MSVC on `windows-2025-vs2026`.

The workflow verifies that `rustc -vV` reports a host triple matching the
artifact target before packaging. After packaging, it runs `xtask
archive-smoke` against the generated archive and uploads the smoke evidence
JSON beside the artifacts.

Run fresh signed artifacts from current `main`:

```sh
gh workflow run release-artifacts.yml --ref main -f sign_artifacts=true
gh run list --workflow release-artifacts.yml --branch main --limit 3
gh run watch <RUN_ID> --exit-status
```

Download and verify:

```sh
rm -rf "target/release-candidate-evidence-<RUN_ID>"
mkdir -p "target/release-candidate-evidence-<RUN_ID>"
gh run download <RUN_ID> --dir "target/release-candidate-evidence-<RUN_ID>"

cargo run -p xtask -- verify-release-artifacts \
  --dir "target/release-candidate-evidence-<RUN_ID>/fileferry-x86_64-unknown-linux-gnu-release-artifacts" \
  --target x86_64-unknown-linux-gnu \
  --expect-signature
```

Repeat the verifier for every intended target directory.

## Installers

The current tested installers are:

```text
scripts/install.sh
scripts/install.ps1
```

`xtask release-package` copies both scripts into the artifact directory and
includes them in `SHA256SUMS`. The scripts install from a local FileFerry
`.tar.gz` archive and are intentionally non-interactive when their archive and
install directory are supplied.

Unix shell example:

```sh
sh ./install.sh \
  --archive ./fileferry-1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz \
  --install-dir "$HOME/.local/bin"
```

PowerShell example:

```powershell
pwsh -NoLogo -NoProfile -NonInteractive -File ./install.ps1 `
  -Archive ./fileferry-1.0.0-rc.1-x86_64-pc-windows-msvc.tar.gz `
  -InstallDir "$HOME/bin"
```

Checksum behavior:

- If `SHA256SUMS` is next to the archive, the installer verifies the archive
  entry before installing.
- `install.sh` also accepts `--checksum-file`, `--checksum`, `--no-checksum`,
  and `--dry-run`.
- `install.ps1` accepts `-ChecksumFile`, `-Checksum`, `-NoChecksum`, and
  `-DryRun`.
- A checksum mismatch fails without writing the destination binary.

PowerShell-on-macOS evidence proves installer script behavior. It is not by
itself a Windows support claim; Windows release wording still depends on the
matching CI, platform tests, release artifacts, and smoke evidence.

## GitHub Release Publication

The tag and GitHub release must point to the exact commit used for the verified
fresh artifact run.

Preferred shape:

```sh
commit="$(git rev-parse HEAD)"
git tag -s v1.0.0-rc.1 "$commit" -m "FileFerry 1.0.0-rc.1"
git push origin v1.0.0-rc.1

gh release create v1.0.0-rc.1 <artifact-files...> \
  --title "FileFerry 1.0.0-rc.1" \
  --notes-file docs/release-notes-1.0.0-rc.1.md \
  --prerelease \
  --target "$commit"
```

If local GPG signing is unavailable, stop. Do not create an unsigned tag unless
Stephen explicitly authorizes it.

Attach the verified archives, `SHA256SUMS`, Sigstore bundles, SBOMs, release
manifests, archive-smoke JSON files, `install.sh`, and `install.ps1` from the
fresh artifact run.

Do not publish a release from uncommitted changes. Do not publish artifacts
whose binary version, commit, checksum, or smoke-test evidence cannot be tied
back to the release candidate.

## Release Notes

Release notes must be written for users and operators. They should include:

- Upgrade impact.
- Repository-format compatibility.
- Security-relevant changes.
- Known limitations.
- Artifact targets and artifact names.
- Verification evidence summary.

Release notes must not include unsupported platform claims or attribution to
automation tools.

The `1.0.0-rc.1` notes are in
[`docs/release-notes-1.0.0-rc.1.md`](release-notes-1.0.0-rc.1.md).
