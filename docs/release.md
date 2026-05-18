# Release

FileFerry does not have v1 release artifacts yet. This document is the manual
release equivalent until a dedicated tool such as `cargo-dist` is adopted.

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

## Manual Release Shape

The manual process is intentionally explicit:

1. Confirm the release candidate commit.
2. Run the full verification gate.
3. Build each target from the release candidate.
4. Package only the expected binaries and license/readme files.
5. Generate checksums.
6. Sign the checksum manifest or archives.
7. Generate SBOM and auditable metadata.
8. Run archive smoke tests on every claimed target.
9. Publish artifacts and release notes from the same commit.

Example local host build:

```sh
cargo build --workspace --release
target/release/ferry version
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
