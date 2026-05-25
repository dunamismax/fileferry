# BUILD.md

Active build tracker for FileFerry and its `ferry` command.

`README.md` describes the public product. `AGENTS.md` holds durable repo
operating rules. This file stays short enough for agents to load quickly and is
currently focused on the next multi-pass effort: metadata restore.

Last rewritten: 2026-05-25.

---

## Current Baseline

FileFerry is an all-Rust workspace with the crate boundaries listed in
`AGENTS.md`, CLI binary `ferry`, public homepage binary `fileferry-web`, `just`
verification recipes, GitHub Actions CI, release packaging automation, and the
v0 repository format documentation/fixtures needed for the current v1 release.

Implemented and tested in current main:

- `ferry init`, `backup`, `restore`, `snapshots`, `ls`, `find`, `diff`,
  `repo`, `doctor`, `check`, `forget`, `prune`, `policy`, `completion`, and
  `version`.
- Key add, marker-based key remove, unlock rotation, full repository rekey,
  encrypted recovery export, and recovery import as a new external key slot.
- Encrypted local and S3-compatible repositories through the shared object
  storage pipeline.
- Encrypted, compressed, deduplicated snapshot creation.
- Snapshot selection by latest, id, and tag.
- Path-scoped restore with destination safety checks, overwrite policy,
  dry-run reporting, byte verification, Unix symlink creation, regular-file
  and directory modified timestamps, regular-file and directory Unix mode bits
  where representable, and Unix ownership verification without `chown`.
- Full repository check plus deterministic count/percentage data subsets.
- Marker-only `forget`, explicit encrypted stored-policy application by policy
  id, and recoverable two-phase `prune`.
- Encrypted command leases for current mutation paths.
- CLI exit-code families, JSON envelopes, JSONL event ordering, and
  stdout/stderr separation documented and regression-tested for the implemented
  command surface.
- Secret-leakage canaries for current human, JSON, JSONL, config parse, S3
  repository URL, S3 endpoint, key-management, recovery export/import, and
  opt-in live-S3 integration output paths.
- S3-compatible hardening evidence for capability probe, retry policy, prefix
  listing surprises, missing objects, permission denial, prune resume, and
  missing-candidate handling, plus gated live provider drills.
- Platform metadata scaffolding and per-target tests for the documented current
  metadata scope in `fileferry-platform`.
- `xtask release-package`, `archive-smoke`, and
  `verify-release-artifacts` for target archives, checksums, Sigstore checksum
  bundles, CycloneDX SBOMs, release manifests, cargo-auditable metadata,
  archive-smoke JSON evidence, and installer scripts.
- CI and release-artifact evidence for the intended `1.0.0` target matrix:
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
  `x86_64-apple-darwin`, `aarch64-apple-darwin`, and
  `x86_64-pc-windows-msvc`.

V1 scope is complete. Platform wording must stay limited to observed CI,
tests, artifacts, and smoke evidence.

---

## Active Objective - Metadata Restore

Goal: make metadata restore complete, honest, and test-backed over multiple
coding passes.

Current restore behavior is the starting line, not the finish line:

- Regular-file and directory modified times are applied after content writes.
- Regular-file and directory Unix permission bits are applied where
  representable, currently limited to `0o777` bits on Unix destinations.
- Regular-file and directory Unix ownership is observed after restore and
  reported as applied only when UID/GID already match; FileFerry does not call
  `chown`.
- Symlink targets are restored on Unix, but symlink timestamps and symlink
  Unix mode/ownership are warnings.
- Creation/birth timestamps, xattr values, ACL contents, file flags, resource
  forks, Windows attributes, sparse extent maps, special files, alternate data
  streams, and other platform extensions are not restored yet.

Rules for every metadata pass:

- Do not claim support until capture, restore, warning behavior, machine
  output, docs, and tests all agree.
- Preserve destination safety preflight before writes.
- Apply content before metadata, and apply directory metadata after child
  writes when ordering matters.
- Report every selected but unapplied metadata field as a structured
  `RestoreMetadataWarning` with entry id, namespace, field, source platform,
  destination platform, and reason.
- Return partial-success exit code `10` when file content restores correctly
  but one or more selected metadata fields cannot be applied.
- Keep sensitive metadata encrypted in repository objects. Do not introduce
  plaintext paths, names, ownership, attributes, xattrs, ACLs, or backup shape.
- Format v0 schemas are frozen for fixture-covered fields. Capturing new
  metadata values beyond current summary/status fields requires an explicit
  schema/format decision, compatibility plan, documentation, and fixtures.
- Prefer `fileferry-platform` for platform-specific metadata capture,
  representation checks, and application primitives. Core should orchestrate
  restore outcomes; CLI should present them.

---

## Metadata Restore Phases

Agents should complete these phases in order unless Stephen explicitly changes
the priority. It is fine for one coding pass to complete only part of a phase.
Check a box only after code, docs, and tests for that item are updated.

### Phase 0 - Baseline Inventory And Contract

- [x] Read `AGENTS.md`, `README.md`, current `BUILD.md`, `docs/`, and
      task-relevant restore/platform code before planning new metadata work.
- [x] Confirm current restore applies regular-file modified timestamps.
- [x] Confirm current restore applies directory modified timestamps.
- [x] Confirm current restore applies regular-file Unix `0o777` mode bits on
      Unix destinations.
- [x] Confirm current restore applies directory Unix `0o777` mode bits on Unix
      destinations.
- [x] Confirm current restore reports creation/birth timestamps as structured
      warnings.
- [x] Confirm current restore reports symlink timestamps and symlink Unix
      mode/ownership as structured warnings.
- [x] Confirm current restore reports platform-extension status fields such as
      xattrs as warnings when selected and observed or denied.
- [x] Confirm JSON, JSONL, and human restore output preserve stdout/stderr
      rules and partial-success exit code `10`.

### Phase 1 - Current-Scope Hardening

Purpose: make the already-implemented metadata subset difficult to regress.

- [x] Add or tighten focused core tests for file and directory modified-time
      application, including failure/warning behavior when timestamps are
      denied, unsupported, or outside the destination system-time range.
- [x] Add or tighten CLI tests that prove file and directory modified times
      survive backup and restore through the `ferry` binary.
- [x] Add or tighten Unix tests for regular-file and directory mode restore,
      including masking to `0o777` and warning for special bits.
- [x] Add or tighten tests proving directory metadata is applied after child
      writes so restored directory mtimes are not clobbered by restore-created
      children.
- [x] Add or tighten dry-run tests so `metadata_planned`,
      `metadata_applied`, and warnings match non-dry-run semantics without
      writing destination entries.
- [x] Recheck `README.md`, `docs/platform-metadata.md`, and
      `docs/cli-contract.md` after test hardening so public wording matches
      observed behavior exactly.

### Phase 2 - Metadata Outcome Architecture

Purpose: make future metadata fields additive instead of one-off counter and
warning logic.

- [x] Introduce typed restore metadata outcomes that distinguish planned,
      applied, skipped-as-unsupported, denied, unrepresentable, failed, and
      not-yet-implemented fields.
- [x] Derive `metadata_planned`, `metadata_applied`, and
      `metadata_warnings` from those outcomes instead of hand-maintained field
      counts.
- [x] Move platform-specific representation/apply probes toward
      `fileferry-platform` where practical, keeping CLI presentation out of
      library crates.
- [x] Preserve the existing JSON/JSONL `RestoreMetadataWarning` shape unless a
      deliberate CLI contract revision is made.
- [x] Add regression tests proving a new metadata field cannot be selected and
      silently ignored.

### Phase 3 - Portable Timestamp Completion

Purpose: finish portable timestamp behavior before deeper platform extensions.

- [x] Decide and document which timestamp fields are restore targets:
      modified, accessed if added, and creation/birth where representable.
- [ ] If creation/birth timestamp restore is pursued, verify current primary
      platform APIs first and document representability limits for Linux,
      macOS, and Windows.
- [ ] Implement creation/birth timestamp application only where the destination
      platform and filesystem can actually represent it.
- [x] Keep creation/birth timestamp warnings where capture or destination
      representation is unsupported.
- [ ] Add platform-gated tests for successful timestamp restore and warning
      paths on each supported target.
- [ ] Update `docs/platform-metadata.md`, `docs/cli-contract.md`, and known
      limitations after behavior is proven.

### Phase 4 - Unix Metadata Expansion

Purpose: deepen Unix restore without unsafe default ownership behavior.

- [ ] Decide whether Unix ownership changes are ever automatic, opt-in, or
      warning-only. Document the decision before implementation.
- [ ] If ownership restore is implemented, add a non-interactive opt-in and
      tests for permission denied, unprivileged operation, mismatched ids, and
      partial-success reporting.
- [ ] Decide whether special Unix mode bits are restore targets. Keep warnings
      for unsupported or unsafe bits.
- [ ] Add symlink metadata support only through APIs that do not follow the
      symlink target. If unavailable, keep structured warnings.
- [ ] Add Linux/macOS tests for regular files, directories, symlinks, and
      unsupported filesystem behavior.
- [ ] Keep Windows behavior explicit: Unix metadata is unrepresentable unless
      a documented Windows mapping is implemented and tested.

### Phase 5 - Platform Extension Values And Format Planning

Purpose: move beyond status/count scaffolding only after the format decision is
explicit.

- [ ] Write a design note or docs update for storing metadata extension values
      without leaking sensitive details in plaintext.
- [ ] Decide whether new metadata value storage requires a new repository
      format version, feature flag, or manifest schema. Add fixtures before
      declaring compatibility.
- [ ] Implement xattr name/value capture and restore for supported Unix
      platforms, with filters for known non-restorable implementation details.
- [ ] Implement ACL capture and restore only after choosing platform-specific
      schemas and failure behavior.
- [ ] Implement macOS file flags and resource fork capture/restore where
      APIs and tests prove behavior.
- [ ] Implement Windows attribute and owner metadata capture/restore where
      APIs and tests prove behavior.
- [ ] Implement sparse-file extent capture/restore only after deciding whether
      sparse layout is a metadata field, content-storage concern, or both.
- [ ] Keep unsupported, denied, or not-yet-implemented extension values as
      structured warnings until value-level restore is proven.

### Phase 6 - Cross-Platform Evidence

Purpose: turn implementation into supportable behavior.

- [ ] Add Linux CI coverage for every metadata field claimed on Linux.
- [ ] Add macOS CI coverage for every metadata field claimed on macOS.
- [ ] Add Windows CI coverage for every metadata field claimed on Windows.
- [ ] Add local restore drills that compare source and restored metadata for
      the implemented field set.
- [ ] Add S3-compatible restore drills only when metadata behavior differs
      from local restore or when storage behavior could affect restore order.
- [ ] Update `docs/operations.md` with dated evidence for each completed
      metadata drill.

### Phase 7 - Documentation And Release Readiness

Purpose: make the public story match the tested behavior.

- [ ] Update `README.md` known limits after each metadata field graduates from
      warning-only to restored.
- [ ] Update `docs/platform-metadata.md` with exact capture/restore behavior,
      representability limits, and warning reasons.
- [ ] Update `docs/cli-contract.md` if metadata output fields, warning
      fields, or exit-code behavior change.
- [ ] Update `docs/repository-format.md` and fixtures for any new
      compatibility-facing metadata fields.
- [ ] Update release notes before publishing a release that changes metadata
      behavior.
- [ ] Run the normal workspace gate before marking metadata restore complete:
      `just fmt`, `just check`, `just test`, and `just build`.

---

## Known Limits Not To Overclaim

- Restore currently applies only the implemented metadata subset. Other
  captured or future metadata must be reported as warnings, not silently
  claimed restored.
- Current normal metadata capture does not restore xattr values, ACL contents,
  file flags, resource forks, Windows attributes, sparse extent maps, symlink
  metadata, accessed timestamps, or creation/birth timestamps.
- Unix UID/GID ownership is verified, not changed.
- Unix special mode bits are warned, not restored.
- `ferry key rotate` currently rotates unlock access by adding a new slot and
  marker-removing selected externally added slots. `ferry key rekey` is the
  separate full repository master-key rewrite path.
- `ferry find` searches encrypted snapshot metadata after repository unlock.
  It does not search file contents or read chunk data.
- `ferry diff` compares encrypted snapshot metadata after repository unlock.
  It uses manifest chunk references for regular-file content identity, but it
  does not read chunk data or compare file contents byte-by-byte.
- `ferry repo` is inspection and metadata/state verification only. It does not
  repair repositories, verify chunk data, or expose decrypted snapshot shape in
  default output.
- `ferry doctor` is diagnostic-only. It reports safe health summaries by
  default, can opt into chunk-data reads or aggregate object-family counts, and
  does not repair repositories or expose decrypted snapshot shape.
- `ferry policy` manages encrypted repository-local policy config objects.
  `ferry forget --policy <POLICY_ID>` applies one selected stored policy
  explicitly; stored policies are not chosen implicitly.
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
