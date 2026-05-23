# BUILD.md

Active build tracker for FileFerry and its `ferry` command.

`README.md` describes the target product. `AGENTS.md` holds durable repo
operating rules. This file is intentionally short: it exists to help agents
pick the next useful build task without loading a project history.

Last condensed: 2026-05-23.

---

## How To Use This File

- Treat checked items as current repo state, not release claims.
- Treat unchecked items as the active backlog to v1.
- Do not add session changelogs here. Put durable technical facts in `docs/`
  and durable agent rules in `AGENTS.md`.
- When a milestone is completed, collapse it to one short status bullet and
  keep only the next open work.
- Prefer finishing the first open milestone that can be completed honestly in
  the current session.

---

## Current Baseline

FileFerry is pre-v1. It is an all-Rust workspace with the planned crate
boundaries, CLI binary `ferry`, public homepage binary `fileferry-web`, `just`
verification recipes, and GitHub Actions CI.

Implemented and tested at the current pre-v1 level:

- `ferry version` and `ferry completion`.
- Config discovery, profiles, environment precedence, redacted diagnostics,
  and human/JSON/JSONL output envelopes.
- Encrypted local and S3-compatible repository initialization.
- Local and S3-compatible `backup`, `snapshots`, `ls`, `restore`, `check`,
  `forget`, `prune`, and key-management command paths through the shared
  encrypted object-store pipeline.
- Encrypted, compressed, deduplicated snapshot creation.
- Snapshot selection by latest, id, and tag.
- Path-scoped restore with destination safety checks, overwrite policy,
  dry-run reporting, byte verification, Unix symlinks, regular-file and
  directory modified timestamps, regular-file and directory Unix mode bits
  where representable, and Unix ownership verification without `chown`.
- Full repository check plus deterministic count/percentage data subsets.
- Marker-only `forget` and recoverable two-phase `prune`.
- Key add, marker-based key remove, limited unlock rotation, and encrypted
  recovery export. Recovery import, full repository rekey, and bootstrap-slot
  removal are not implemented.
- Encrypted command leases for mutation paths currently covered by backup,
  forget, prune, and key-management commands.
- Frozen format v0 compatibility contract for documented current object
  families, format v0 security and repository-format docs, current crypto
  primitives, local/S3 storage abstraction, retry/timeout/concurrency policy
  wrapper, tested retention parser, fake object store, and platform metadata
  scaffolding.
- Golden repository-format fixture slices for bootstrap/key slots/removal
  markers, recovery export, committed snapshot data, forget/prune state,
  policy/config, upload state, migration detection, lease state, and strict
  unknown-field rejection across fixture-covered current v0 JSON shapes,
  including manifest platform metadata.
- CLI exit-code families, JSON envelopes, JSONL event ordering, and
  stdout/stderr separation are documented and regression-tested for the
  implemented command surface.
- Secret-leakage canaries cover current human, JSON, JSONL, config parse, S3
  repository URL, S3 endpoint, S3 config debug, key-management, recovery
  export, and opt-in live-S3 integration output paths.
- S3-compatible hardening evidence covers the required storage capability
  probe, storage-policy retry for retryable write/read/delete/list failures,
  non-retry permission denial, configured prefix listing surprises, prune
  resume, and missing-candidate handling.
- A local restore release drill now passes through the `ferry` binary for
  real local snapshots, covering full, path-scoped, and latest restores plus a
  full repository check.
- Local release-artifact groundwork now exists through `xtask
  release-package`: host builds can be smoke-tested, checksummed, packaged
  with README/LICENSE, emitted with a manifest, generated with a CycloneDX SBOM,
  archive-smoke-tested from the packaged archive, and built with
  `cargo-auditable`; `xtask verify-release-artifacts` parses and verifies the
  per-target artifact directory evidence before upload in the manual workflow.
  The manual GitHub workflow covers the current native hosted x86_64 Linux GNU,
  ARM64 Linux GNU, x86_64 macOS, ARM64 macOS, and x86_64 Windows MSVC matrix
  and can additionally sign checksum manifests with Sigstore keyless signing
  when run.
- Current signed `1.0.0-rc.1` release-candidate workflow evidence for commit
  `c8c82913eb923bed6b070f0961e56815132bb5ba` exists in GitHub run
  `26319862678`: the manual release-artifacts workflow completed package,
  archive-smoke, verifier, signing, and upload steps for x86_64 Linux GNU,
  ARM64 Linux GNU, x86_64 macOS, ARM64 macOS, and x86_64 Windows MSVC native
  hosted targets on 2026-05-23. Uploaded artifacts were downloaded under
  `target/release-candidate-evidence-26319862678/` and locally re-verified
  with `xtask verify-release-artifacts --expect-signature` for all five target
  directories. Each target directory contained the archive, checksums, Sigstore
  checksum bundle, CycloneDX SBOM, release manifest, archive-smoke JSON
  evidence, and installer scripts. Every manifest recorded version
  `1.0.0-rc.1` and commit `c8c82913eb923bed6b070f0961e56815132bb5ba`. This is
  workflow evidence only, not a published release or platform support claim.
- Tested local-archive install paths now exist through `scripts/install.sh` and
  `scripts/install.ps1`; `xtask release-package` copies them beside the archive,
  includes them in `SHA256SUMS`, and the installers verify archive checksums
  before writing `ferry`. PowerShell evidence is from `pwsh` on macOS and is not
  a Windows support claim.
- CI is configured to run the normal Rust gate on Ubuntu Linux x86_64 GNU,
  Ubuntu Linux ARM64 GNU, macOS Intel, macOS ARM64, and Windows x86_64 MSVC
  hosted runners. Completed passing jobs are host build/test evidence only, not
  platform support claims. Commit
  `c8c82913eb923bed6b070f0961e56815132bb5ba` passed this workflow in GitHub run
  `26319699007` on 2026-05-23.
- Per-target platform metadata tests exist for the documented current metadata
  scope in `fileferry-platform`: Linux and macOS cfg-gated tests assert source
  platform, Unix metadata, timestamps, xattr status, and unsupported extension
  scaffolding; Windows cfg-gated tests assert Windows source platform, absence
  of Unix metadata, and unsupported extension scaffolding. These tests run
  through the hosted CI matrix above and do not claim restoration of metadata
  that is only reported as unsupported or not-yet-restored.
- Live Backblaze B2 S3-compatible drills passed on 2026-05-22 under an
  isolated private development prefix for storage round-trip, CLI init,
  `backup -> snapshots -> ls -> restore -> check`, missing-manifest failure,
  retention/key management, and prune dry-run/sweep behavior. This is current
  provider evidence, not a broad S3-provider support claim.
- `fileferry-web` serves the public `fileferry.app` homepage with Axum,
  server-rendered Leptos views, static CSS, and `/healthz`.

Do not claim supported platforms yet. Current CI and release-artifact workflow
evidence are not the full support bar, and published v1 release artifacts do not
exist.

---

## Active Milestones

### Milestone H - Format Fixtures And Compatibility Freeze

Status: complete at the current pre-v1 level. Format v0's compatibility
contract is frozen for the object families and fields listed in
`docs/repository-format.md`; future fields, new object families, and migration
behavior require an explicit new format version or documented feature gate with
fixtures.

### Milestone I - V1 Release Hardening

Goal: build the release evidence path after local and S3 repository behavior
meet the v1 scope.

Finish this milestone by proving the release candidate, not by polishing
status text:

- [x] Run documented local restore drills from real FileFerry snapshots.
- [x] Run documented S3-compatible restore drills against an isolated test
      prefix and record current provider evidence.
- [ ] Add CI builds and tests for every platform that README or release docs
      call supported.
- [x] Add per-target platform metadata tests before making support claims.
- [x] Audit logs, errors, JSON, JSONL, tests, and docs for secret leakage.
- [x] Audit exit codes, JSON schemas, JSONL event order, and stdout/stderr
      separation as compatibility surfaces.
- [x] Add release artifacts, checksums, signatures, SBOM,
      archive-smoke evidence, and `cargo-auditable` metadata for the exact
      `1.0.0-rc.1` candidate on the intended target matrix.
- [x] Add tested Unix shell and PowerShell install paths.
- [x] Run smoke tests on every intended target artifact in the manual
      release-artifacts workflow.
- [x] Update README, release docs, and release notes to match the exact
      `1.0.0-rc.1` release candidate.

Non-goals:

- Tagging v1 before the exact release candidate passes.
- Claiming unsupported platforms.
- Adding GUI, TUI, daemon, server, scheduler, SaaS, mobile, FUSE, or
  repository-compatibility behavior.

---

## V1 Scope Checklist

Core product:

- [x] Rust workspace with target crate boundaries.
- [x] `ferry init` for encrypted local and S3-compatible repositories.
- [x] `ferry backup` for encrypted, compressed, deduplicated snapshots.
- [x] `ferry restore` by latest, snapshot id, tag, and path.
- [x] `ferry snapshots` and `ferry ls`.
- [x] `ferry check` for metadata, full data, and deterministic data subsets.
- [x] `ferry forget` and recoverable two-phase `ferry prune`.
- [x] Key add/remove/rotate/export-recovery paths.
- [x] Stable config profiles and environment variables.
- [x] Shell completion generation.
- [x] Format v0 freeze decision completed.
- [x] S3-compatible capability-probe, retry, resume, listing-surprise, and
      permission-error evidence completed for the release candidate.
- [x] Exit codes, JSON, and JSONL contracts fully documented and tested for
      the implemented command surface.
- [x] Local restore drills completed from real FileFerry snapshots.
- [x] S3-compatible restore drills completed against an isolated test prefix.

Security and format:

- [x] Client-side encryption for repository data and sensitive metadata.
- [x] Authenticated encrypted objects.
- [x] Wrong-password, wrong-key, tamper, corruption, replay, and malformed
      metadata coverage for current object paths.
- [x] Plaintext bootstrap fields documented.
- [x] Key hierarchy, KDF parameters, AEAD choice, and recovery export behavior
      documented.
- [x] Secret-leakage audit completed for release candidate output and tests.
- [x] Format fixtures declared complete or remaining blockers documented.

Platform and release:

- [x] Platform metadata target documented.
- [x] Current host-observed metadata behavior and restore-warning contract
      tested.
- [x] Per-target platform metadata tests exist for the documented current
      metadata scope on Linux, macOS, and Windows through cfg-gated
      `fileferry-platform` tests in the hosted CI matrix.
- [ ] CI, tests, and artifacts exist for every claimed supported platform.
- [x] Release archives, checksums, signatures, SBOM, and audit metadata exist
      for the exact `1.0.0-rc.1` release candidate.
- [x] Unix shell and PowerShell install paths are tested.

Post-v1 or optional:

- [ ] `ferry find`.
- [ ] `ferry diff`.
- [ ] `ferry repo`.
- [ ] `ferry policy`.
- [ ] `ferry doctor`.
- [ ] Recovery import.
- [ ] Full repository rekey.
- [ ] Broader storage providers.

Out of scope for v1: GUI, TUI, daemon, server, scheduler, SaaS, mobile app,
FUSE mount, and compatibility with existing backup repository formats.

---

## Known Limits Not To Overclaim

- No platform is supported until CI, relevant tests, and release artifacts
  exist for that platform.
- Restore applies only the implemented metadata subset. Other captured or
  future metadata must be reported as warnings, not silently claimed restored.
- Current normal metadata capture does not restore xattr values, ACL contents,
  file flags, resource forks, Windows attributes, sparse extent maps, symlink
  metadata, or creation/birth timestamps.
- Key rotation currently rotates unlock access by adding a new slot and
  marker-removing selected externally added slots. It does not rewrite
  repository data with a new master key.
- Command leases cover current mutation paths. They are not a full stale-lease
  repair system, repository maintenance framework, or broad concurrent-backup
  proof.
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

Expected release evidence:

- Crypto and key-management tests.
- Repository format golden fixture tests.
- Corruption, tamper, replay, and wrong-key tests.
- Fake object store idempotency and interruption tests.
- Local backend integration tests.
- S3-compatible integration tests against an isolated test bucket or emulator.
- CLI golden tests for help, JSON, JSONL, and exit codes.
- Platform metadata tests on every supported platform.
- Restore drills from real snapshots.
- Release artifact smoke tests.

---

## External Sources To Re-check

Use current primary sources before implementation work that depends on external
behavior:

- Rust stable release, edition, MSRV, Cargo workspace, and target support.
- clap, tokio, serde, tracing, miette, color-eyre, object_store, OpenDAL,
  fastcdc, blake3, zstd, secrecy, zeroize, and Argon2id documentation.
- Current cryptographic guidance for AEADs, KDF parameters, nonce strategy,
  key rotation, and authenticated metadata.
- Windows, macOS, and Linux filesystem metadata and path behavior.
- S3-compatible storage behavior, multipart uploads, retry semantics,
  consistency guarantees, and provider limits.
- cargo-dist, signing, SBOM, `cargo-auditable`, Homebrew, Scoop, and WinGet
  release documentation.

Trust current primary docs and observed behavior over this file.
