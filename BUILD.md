# BUILD.md

Active build tracker for FileFerry and its `ferry` command.

`README.md` describes the public product. `AGENTS.md` holds durable repo
operating rules. This file stays short so agents can find the next useful
build task without loading project history.

Last condensed: 2026-05-23.

---

## Current Baseline

FileFerry is an all-Rust workspace with the crate boundaries listed in
`AGENTS.md`, CLI binary `ferry`, public homepage binary `fileferry-web`, `just`
verification recipes, GitHub Actions CI, release packaging automation, and the
v0 repository format documentation/fixtures needed for the current release
candidate.

Implemented and tested in current main:

- `ferry init`, `backup`, `restore`, `snapshots`, `ls`, `check`, `forget`,
  `find`, `prune`, `completion`, and `version`.
- Key add, marker-based key remove, limited unlock rotation, encrypted
  recovery export, and recovery import as a new external key slot. Full
  repository rekey and bootstrap-slot removal are not implemented.
- Encrypted local and S3-compatible repositories through the shared object
  storage pipeline.
- Encrypted, compressed, deduplicated snapshot creation.
- Snapshot selection by latest, id, and tag.
- Path-scoped restore with destination safety checks, overwrite policy,
  dry-run reporting, byte verification, Unix symlinks, regular-file and
  directory modified timestamps, regular-file and directory Unix mode bits
  where representable, and Unix ownership verification without `chown`.
- Full repository check plus deterministic count/percentage data subsets.
- Marker-only `forget` and recoverable two-phase `prune`.
- Encrypted command leases for current mutation paths.
- CLI exit-code families, JSON envelopes, JSONL event ordering, and
  stdout/stderr separation documented and regression-tested for the
  implemented command surface.
- Secret-leakage canaries for current human, JSON, JSONL, config parse, S3
  repository URL, S3 endpoint, key-management, recovery export/import, and
  opt-in live-S3 integration output paths.
- S3-compatible hardening evidence for capability probe, retry policy, prefix
  listing surprises, missing objects, permission denial, prune resume, and
  missing-candidate handling.
- Local restore drills and live Backblaze B2 S3-compatible drills under an
  isolated private development prefix. This is provider evidence, not a broad
  S3-provider support claim.
- Platform metadata scaffolding and per-target tests for the documented
  current metadata scope in `fileferry-platform`.
- `xtask release-package`, `archive-smoke`, and
  `verify-release-artifacts` for target archives, checksums, Sigstore checksum
  bundles, CycloneDX SBOMs, release manifests, cargo-auditable metadata,
  archive-smoke JSON evidence, and installer scripts.
- CI and the manual release-artifacts workflow cover the intended
  `1.0.0-rc.1` release-candidate target matrix:
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
  `x86_64-apple-darwin`, `aarch64-apple-darwin`, and
  `x86_64-pc-windows-msvc`.
- `fileferry-web` serves the public `fileferry.app` homepage with Axum,
  server-rendered Leptos views, static CSS, and `/healthz`.

Do not claim final v1.0.0. `1.0.0-rc.1` is a release candidate. Platform
wording must stay limited to observed CI, tests, artifacts, and smoke evidence.

---

## Active Milestones

### Milestone H - Format Fixtures And Compatibility Freeze

Status: complete at the current v0 level. Format v0's compatibility contract
is frozen for the object families and fields listed in
`docs/repository-format.md`. Future fields, new object families, and migration
behavior require an explicit new format version or documented feature gate with
fixtures.

### Milestone I - 1.0.0-rc.1 Publication

Status: release candidate scope is complete. Publication requires the exact
tagged commit to have:

- Passing CI on the intended hosted target matrix.
- Signed release artifacts from that same commit.
- Local `xtask verify-release-artifacts --expect-signature` verification for
  every intended target artifact directory.
- A prerelease GitHub release named `FileFerry 1.0.0-rc.1`.
- Release notes that clearly identify the build as a release candidate.

Non-goals:

- Tagging or publishing final v1.0.0.
- Claiming unsupported platform behavior.
- Adding GUI, TUI, daemon, server, scheduler, SaaS, mobile, FUSE, or
  repository-compatibility behavior.

---

## V1 Scope Checklist

Core product:

- [x] Rust workspace with target crate boundaries.
- [x] Encrypted local and S3-compatible repository initialization.
- [x] Encrypted, compressed, deduplicated backup snapshots.
- [x] Restore by latest, snapshot id, tag, and path.
- [x] `snapshots`, `ls`, `find`, `check`, `forget`, and recoverable two-phase
      `prune`.
- [x] Key add/remove/rotate/export-recovery/import-recovery paths.
- [x] Stable config profiles and environment variables.
- [x] Shell completion generation.
- [x] Format v0 freeze decision completed.
- [x] Exit codes, JSON, and JSONL contracts documented and tested for the
      implemented command surface.
- [x] Local restore drills and S3-compatible restore drills completed for the
      release candidate.

Security and format:

- [x] Client-side encryption for repository data and sensitive metadata.
- [x] Authenticated encrypted objects.
- [x] Wrong-password, wrong-key, tamper, corruption, replay, and malformed
      metadata coverage for current object paths.
- [x] Plaintext bootstrap fields documented.
- [x] Key hierarchy, KDF parameters, AEAD choice, and recovery export behavior
      documented.
- [x] Secret-leakage audit completed for release candidate output and tests.
- [x] Format fixtures declared complete for the documented v0 surface.

Platform and release:

- [x] Platform metadata target documented.
- [x] Current host-observed metadata behavior and restore-warning contract
      tested.
- [x] Per-target platform metadata tests exist for the documented current
      metadata scope on Linux, macOS, and Windows through the hosted CI matrix.
- [x] Release-candidate archives, checksums, signatures, SBOMs, manifests,
      cargo-auditable metadata, installer scripts, and archive-smoke evidence
      exist for the intended target matrix when generated by the manual
      release-artifacts workflow.
- [x] Unix shell and PowerShell install paths are tested.

Additional command surface:

- [x] `ferry find`.
- [ ] `ferry diff`.
- [ ] `ferry repo`.
- [ ] `ferry policy`.
- [ ] `ferry doctor`.
- [x] Recovery import.
- [ ] Full repository rekey.
- [ ] Broader storage providers.

Out of scope for v1: GUI, TUI, daemon, server, scheduler, SaaS, mobile app,
FUSE mount, and compatibility with existing backup repository formats.

---

## Known Limits Not To Overclaim

- `1.0.0-rc.1` is a release candidate, not final v1.0.0.
- Restore applies only the implemented metadata subset. Other captured or
  future metadata must be reported as warnings, not silently claimed restored.
- Current normal metadata capture does not restore xattr values, ACL contents,
  file flags, resource forks, Windows attributes, sparse extent maps, symlink
  metadata, or creation/birth timestamps.
- Key rotation currently rotates unlock access by adding a new slot and
  marker-removing selected externally added slots. It does not rewrite
  repository data with a new master key.
- `ferry find` searches encrypted snapshot metadata after repository unlock.
  It does not search file contents or read chunk data.
- Command leases cover current mutation paths. They are not a full stale-lease
  repair system or broad concurrent-backup proof.
- S3-compatible behavior must be described only to the level backed by current
  tests and, for provider claims, current live evidence.

---

## Verification

Docs-only work:

```sh
git diff --check
```

Normal workspace gate:

```sh
just fmt
just check
just test
just build
```

Equivalent expanded gate:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace
```

Use focused tests first when changing a narrow area, then broaden as risk
increases. If a command cannot run, report why and what was verified instead.
