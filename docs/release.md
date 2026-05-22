# Release

FileFerry does not have v1 release artifacts yet. This document defines the
current release-candidate evidence path until a dedicated tool such as
`cargo-dist` is adopted.

The release process must not claim platform support before CI, tests, and
artifacts exist for that target.

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
`README.md`, and `LICENSE`, writes `SHA256SUMS`, writes a release manifest,
generates a CycloneDX SBOM for the `ferry` binary, and smoke-tests the host
binary with `ferry version --json`.

Useful options:

```text
--target <TRIPLE>   Package a specific Rust target triple
--out-dir <DIR>     Write artifacts somewhere other than target/release-artifacts
--auditable         Build with cargo-auditable metadata
--sbom              Generate a CycloneDX JSON SBOM with cargo-cyclonedx
--sign              Sign SHA256SUMS with cosign sign-blob
--skip-smoke        Skip the host binary smoke test
```

`--sign` requires a configured `cosign` identity or key. Local unsigned
artifacts are useful for dry runs, but they are not release artifacts.

The workflow `.github/workflows/release-artifacts.yml` is manual-only. It
builds candidate artifacts with `cargo-auditable`, generates SBOMs with
`cargo-cyclonedx`, and can sign the checksum manifest with Sigstore keyless
signing through GitHub OIDC. A workflow run is release evidence only for the
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
cargo run -p xtask -- release-package --auditable --sbom
tmpdir="$(mktemp -d)"
tar -xzf target/release-artifacts/fileferry-0.0.0-$(rustc -vV | awk '/host:/ {print $2}').tar.gz -C "$tmpdir"
"$tmpdir"/fileferry-0.0.0-$(rustc -vV | awk '/host:/ {print $2}')/ferry version --json
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
