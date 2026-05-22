# BUILD.md

Active build tracker for FileFerry and its `ferry` command.

`README.md` describes the target product. `AGENTS.md` holds durable repo
operating rules. This file is intentionally short: it exists to help agents
pick the next useful build task without loading a project history.

Last condensed: 2026-05-22.

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
- Format v0 security and repository-format docs, current crypto primitives,
  local/S3 storage abstraction, retry/timeout/concurrency policy wrapper,
  tested retention parser, fake object store, and platform metadata scaffolding.
- Golden repository-format fixture slices for bootstrap/key slots/removal
  markers, recovery export, committed snapshot data, forget/prune state,
  policy/config, upload state, migration detection, lease state, and strict
  unknown-field rejection across fixture-covered current v0 JSON shapes.
- A live Backblaze B2 S3-compatible data-path drill has passed for
  `init -> backup -> snapshots -> ls -> restore -> check` under an isolated
  private development prefix. Later S3 retention/key/prune live gates exist but
  are not assumed to have current provider evidence unless rerun.
- `fileferry-web` serves the public `fileferry.app` homepage with Axum,
  server-rendered Leptos views, static CSS, and `/healthz`.

Do not claim supported platforms yet. Current CI is not the full support bar,
and release artifacts do not exist.

---

## Active Milestones

### Milestone H - Format Fixtures And Compatibility Freeze

Goal: freeze repository format `0` only after fixture coverage, documented
compatibility fields, and unsupported-version behavior are explicit.

Current state:

- Fixture slices exist for the major current v0 object families listed in
  `Current Baseline`.
- Fixture tests prove current readers can authenticate and validate those
  bytes and reject representative malformed, tampered, replayed, unsupported,
  and wrong-context variants.
- Strict deserialization now rejects unknown fields in fixture-covered current
  v0 repository JSON shapes.
- The broader format is not frozen merely because fixture slices exist.

Finish this milestone by doing the smallest honest freeze audit:

- [ ] Compare `docs/repository-format.md`, the fixture directory, and current
      read/write code to identify any v0 object family or plaintext field that
      still lacks fixture coverage or compatibility classification.
- [ ] Either add the missing fixture/docs coverage or explicitly mark the
      shape as internal/pre-freeze with a reason.
- [ ] Add or update a test that makes unsupported future format versions and
      unknown current-v0 feature flags fail before repository unlock.
- [ ] Update `docs/repository-format.md` so frozen fields, internal fields,
      plaintext reasons, migration detection, and non-goals are clear.
- [ ] Decide and document whether format `0` is frozen. If not frozen, list
      the exact remaining blockers in this section.

Non-goals:

- Compatibility with restic, rustic, Borg, Kopia, rclone, or any other backup
  format.
- Implementing format migrations before a migration is intentionally designed.
- Adding new storage providers or broad repair behavior.

### Milestone I - V1 Release Hardening

Goal: build the release evidence path after local and S3 repository behavior
meet the v1 scope.

Finish this milestone by proving the release candidate, not by polishing
status text:

- [ ] Run documented local restore drills from real FileFerry snapshots.
- [ ] Run documented S3-compatible restore drills against an isolated test
      prefix and record current provider evidence.
- [ ] Add CI builds and tests for every platform that README or release docs
      call supported.
- [ ] Add per-target platform metadata tests before making support claims.
- [ ] Audit logs, errors, JSON, JSONL, tests, and docs for secret leakage.
- [ ] Audit exit codes, JSON schemas, JSONL event order, and stdout/stderr
      separation as compatibility surfaces.
- [ ] Add release artifacts, checksums, signatures, SBOM, and
      `cargo-auditable` metadata.
- [ ] Add tested Unix shell and PowerShell install paths.
- [ ] Run smoke tests on every claimed platform artifact.
- [ ] Update README, docs, completions, homepage status, and release notes to
      match the exact release candidate.

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
- [ ] Format v0 freeze decision completed.
- [ ] S3-compatible retry, resume, listing-surprise, and permission-error
      evidence completed for the release candidate.
- [ ] Exit codes, JSON, and JSONL contracts fully documented and tested.
- [ ] Restore drills completed for local and S3-compatible repositories.

Security and format:

- [x] Client-side encryption for repository data and sensitive metadata.
- [x] Authenticated encrypted objects.
- [x] Wrong-password, wrong-key, tamper, corruption, replay, and malformed
      metadata coverage for current object paths.
- [x] Plaintext bootstrap fields documented.
- [x] Key hierarchy, KDF parameters, AEAD choice, and recovery export behavior
      documented.
- [ ] Secret-leakage audit completed for release candidate output and tests.
- [ ] Format fixtures declared complete or remaining blockers documented.

Platform and release:

- [x] Platform metadata target documented.
- [x] Current host-observed metadata behavior and restore-warning contract
      tested.
- [ ] CI, tests, and artifacts exist for every claimed supported platform.
- [ ] Release archives, checksums, signatures, SBOM, and audit metadata exist.
- [ ] Unix shell and PowerShell install paths are tested.

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
- OpenBSD remains best-effort until release and CI support are real.
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
- Windows, macOS, Linux, FreeBSD, NetBSD, and OpenBSD filesystem metadata and
  path behavior.
- S3-compatible storage behavior, multipart uploads, retry semantics,
  consistency guarantees, and provider limits.
- cargo-dist, signing, SBOM, `cargo-auditable`, Homebrew, Scoop, WinGet,
  FreeBSD ports, and pkgsrc release documentation.

Trust current primary docs and observed behavior over this file.
