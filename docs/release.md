# Release

FileFerry does not have v1 release artifacts yet. This document defines the
current release-candidate evidence path until a dedicated tool such as
`cargo-dist` is adopted.

The release process must not claim platform support before CI, tests, and
artifacts exist for that target.

## CI Evidence

The normal CI workflow is configured to run the Rust formatting, clippy, test,
and build gate on these hosted runner targets:

- Ubuntu Linux x86_64 GNU host.
- Ubuntu Linux ARM64 GNU host.
- macOS Intel host.
- macOS ARM64 host.
- Windows x86_64 MSVC host.

Completed passing jobs are required release-candidate evidence, but they are
not platform support by themselves. Support still requires the target-specific
release artifact, checksum/signature/SBOM/auditable metadata, archive smoke
evidence, and relevant platform metadata tests for the exact release candidate.

## Observed 1.0.0-rc.1 Candidate Evidence

Current observed workflow evidence for commit
`c8c82913eb923bed6b070f0961e56815132bb5ba`:

- Normal CI passed in GitHub run
  [26319699007](https://github.com/dunamismax/fileferry/actions/runs/26319699007)
  on 2026-05-23 for Ubuntu Linux x86_64 GNU, Ubuntu Linux ARM64 GNU,
  macOS Intel, macOS ARM64, and Windows x86_64 MSVC hosted runners.
- The manual release-artifacts workflow passed with signing enabled in GitHub
  run
  [26319862678](https://github.com/dunamismax/fileferry/actions/runs/26319862678)
  on 2026-05-23 for x86_64 Linux GNU, ARM64 Linux GNU, x86_64 macOS,
  ARM64 macOS, and x86_64 Windows MSVC native hosted targets.
- Each release-artifacts job completed formatting, clippy, tests, packaging,
  archive smoke, artifact-directory verification, and artifact upload steps.
- The uploaded per-target artifact directories were observed as unexpired and
  tied to the same head SHA. Each contained a target archive, `SHA256SUMS`,
  `SHA256SUMS.sigstore.json`, a CycloneDX `*.cdx.json` SBOM, a release
  manifest, an archive-smoke JSON file, `install.sh`, and `install.ps1`.
- The uploaded artifacts were downloaded under
  `target/release-candidate-evidence-26319862678/` and locally re-verified on
  2026-05-23 with `cargo run -p xtask -- verify-release-artifacts --dir
  <artifact-dir> --target <target> --expect-signature` for all five intended
  targets.
- Each manifest recorded version `1.0.0-rc.1` and commit
  `c8c82913eb923bed6b070f0961e56815132bb5ba`.

This evidence is tied to that exact commit and those workflow runs. It is not a
published v1 release and not a support claim.

Release-runner drift was rechecked on 2026-05-23 against current GitHub
sources. The release workflow now uses `actions/upload-artifact@v7`, the latest
stable major observed in the upstream action releases, and uses the concrete
`windows-2025-vs2026` hosted runner label for the Windows x86_64 MSVC job
instead of the ambiguous `windows-latest` label. The native host triple guard
remains in the release workflow.

## Preconditions

- The release candidate is built from a clean checkout on the intended tag.
- `BUILD.md` has no unchecked v1 blocker that is being claimed as complete.
- No secrets, `.env` files, private repositories, production logs, recovery
  exports, or real backup data are present in the tree.
- The configured Git identity belongs to `dunamismax`.
- The exact release candidate has passed:

```sh
just fmt
just check
just test
just build
```

## Artifact Policy

Each supported target needs:

- A target-specific archive containing the `ferry` binary.
- Checksums for every archive.
- A signature for the checksum manifest or each archive.
- SBOM output.
- `cargo-auditable` metadata or a documented replacement.
- A smoke-test record showing the archive binary starts and reports `ferry
  version`.

Targets without passing CI and a smoke-tested artifact are not supported
targets for that release.

## Local Artifact Task

The retained local packaging entrypoint is:

```sh
cargo run -p xtask -- release-package --auditable --sbom
```

By default this builds the host `ferry` binary, packages only the binary,
`README.md`, and `LICENSE`, copies the Unix shell and PowerShell installer
scripts beside the archive, writes `SHA256SUMS`, writes a release manifest,
generates a CycloneDX SBOM for the `ferry` binary, and smoke-tests the host
binary with `ferry version --json`. For host-target packages, it also verifies
the archive entry in `SHA256SUMS`, extracts the archive, runs the packaged
binary with `ferry version --json`, and records archive-smoke evidence in the
release manifest. Installer smoke paths run when the relevant installer script
and interpreter are available on the host running the task.

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
  --out target/release-artifacts/archive-smoke.json
```

`archive-smoke` verifies the archive against `SHA256SUMS` when it is present
beside the archive, extracts the archive, runs the packaged binary with
`ferry version --json`, verifies cargo-auditable metadata on the packaged
binary when `--expect-auditable` is supplied, and optionally runs available
installer scripts from `--installers-dir` before smoke-testing the installed
binary. Use `--target <TRIPLE>` to make target mismatch failures explicit. Use
`--checksum-file <FILE>` to point at a checksum manifest in another location.
Use `--no-checksum` only for local diagnostics; unchecked archives are not
release evidence.

The retained artifact-directory verifier is:

```sh
cargo run -p xtask -- verify-release-artifacts \
  --dir target/release-artifacts \
  --target "$(rustc -vV | awk '/host:/ {print $2}')" \
  --expect-signature
```

It parses the release manifest, CycloneDX SBOM, Sigstore bundle when expected,
and archive-smoke evidence JSON, verifies target ownership, and checks the
`SHA256SUMS` entries for the archive, manifest, SBOM, and installer scripts.

## Installer Scripts

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
version="1.0.0-rc.1"
host="$(rustc -vV | awk '/host:/ {print $2}')"
sh target/release-artifacts/install.sh \
  --archive "target/release-artifacts/fileferry-${version}-${host}.tar.gz" \
  --install-dir "$HOME/.local/bin"
```

PowerShell example:

```powershell
$version = "1.0.0-rc.1"
$hostTriple = rustc -vV | Select-String '^host: ' | ForEach-Object { $_.Line.Split(' ')[1] }
pwsh -NoLogo -NoProfile -NonInteractive -File target/release-artifacts/install.ps1 `
  -Archive "target/release-artifacts/fileferry-${version}-$hostTriple.tar.gz" `
  -InstallDir "$HOME/.local/bin"
```

Checksum behavior:

- If `SHA256SUMS` is next to the archive, the installer verifies the archive
  entry before installing.
- `install.sh` also accepts `--checksum-file`, `--checksum`, `--no-checksum`,
  and `--dry-run`.
- `install.ps1` accepts `-ChecksumFile`, `-Checksum`, `-NoChecksum`, and
  `-DryRun`.
- A checksum mismatch fails without writing the destination binary.

Current evidence:

- `cargo test -p xtask` exercises archive smoke option parsing, checksum
  verification, archive extraction, packaged-binary execution, and the Unix
  installer smoke path with a fixture archive.
- `cargo test -p xtask install` exercises Unix install, Unix dry-run, Unix
  checksum mismatch, PowerShell install, and PowerShell checksum mismatch.
- Local macOS verification ran `pwsh --version` and executed the PowerShell
  installer with `pwsh`.

PowerShell-on-macOS evidence proves installer script behavior. It is not a
Windows platform support claim; Windows support still requires CI, platform
tests, release artifacts, and smoke evidence for the claimed target.

The workflow `.github/workflows/release-artifacts.yml` is manual-only. It
builds candidate artifacts with `cargo-auditable`, generates SBOMs with
`cargo-cyclonedx`, and can sign the checksum manifest with Sigstore keyless
signing through GitHub OIDC. The current native hosted matrix is:

- Linux x86_64 GNU on `ubuntu-latest`.
- Linux ARM64 GNU on `ubuntu-24.04-arm`.
- macOS x86_64 on `macos-15-intel`.
- macOS ARM64 on `macos-15`.
- Windows x86_64 MSVC on `windows-2025-vs2026`.

The workflow verifies that `rustc -vV` reports a host triple matching the
artifact target before packaging. After packaging, it runs `xtask
archive-smoke` against the generated archive and uploads the smoke evidence
JSON beside the artifacts. A workflow run is release evidence only for the
exact commit, target, artifacts, signatures, SBOMs, checksums, and smoke tests
that it actually produced.

## Manual Release Shape

The manual process is intentionally explicit:

1. Confirm the release candidate commit.
2. Run the full verification gate.
3. Run the release artifact workflow or `xtask release-package` for each
   intended target.
4. Confirm each target artifact was built with auditable metadata.
5. Confirm each target artifact has a checksum, signature bundle, SBOM, and
   release manifest.
6. Run archive smoke tests on every claimed target.
7. Publish artifacts and release notes from the same commit.

Example local host build:

```sh
version="1.0.0-rc.1"
host="$(rustc -vV | awk '/host:/ {print $2}')"
cargo run -p xtask -- release-package --auditable --sbom
cargo run -p xtask -- archive-smoke \
  --archive "target/release-artifacts/fileferry-${version}-${host}.tar.gz" \
  --target "${host}" \
  --installers-dir target/release-artifacts \
  --expect-auditable
```

Do not publish a release from uncommitted changes. Do not publish artifacts
whose binary version, commit, checksum, or smoke-test evidence cannot be tied
back to the release candidate.

## Release Notes

Release notes must be written for users and operators. They should include:

- Upgrade impact.
- Repository-format compatibility.
- Security-relevant changes.
- Known limitations.
- Supported platforms with artifact names.
- Verification evidence summary.

Release notes must not include AI attribution or unsupported platform claims.

### 1.0.0-rc.1 Candidate Notes

Notes for commit `c8c82913eb923bed6b070f0961e56815132bb5ba`:

- FileFerry remains unpublished. These notes describe a release candidate only;
  no v1 tag or published release exists yet.
- The `ferry` package version and workspace crate versions are stamped as
  `1.0.0-rc.1`.
- This candidate exercises the current encrypted local and S3-compatible
  repository paths, restore drills, retention/prune flows, key-management
  surface, JSON/JSONL contracts, secret-redaction canaries, installer scripts,
  and release artifact tooling already tracked in `BUILD.md`.
- Repository format v0 is frozen for the currently documented object families
  and fixture-covered JSON shapes. Future format changes still require an
  explicit version or documented feature gate with fixtures.
- Intended artifact scope is `x86_64-unknown-linux-gnu`,
  `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`,
  `aarch64-apple-darwin`, and `x86_64-pc-windows-msvc`.
- CI passed in GitHub run `26319699007`. Signed artifacts, checksums, SBOMs,
  cargo-auditable metadata, archive-smoke evidence, and installer scripts were
  produced in GitHub run `26319862678`.
- Downloaded artifacts were locally verified under
  `target/release-candidate-evidence-26319862678/` with
  `xtask verify-release-artifacts --expect-signature` for all intended
  targets.
- The candidate is not a support claim. Platform support still requires the
  explicit publication decision and release wording for the selected target set.
