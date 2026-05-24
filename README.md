# FileFerry

Encrypted backups. Same everywhere.

FileFerry is an all-Rust backup CLI. The primary command is `ferry`.
It creates encrypted, compressed, deduplicated snapshots and restores them from
local filesystem or S3-compatible object storage repositories.

FileFerry is CLI-only by design. It is not a GUI, TUI, daemon, scheduler,
server, SaaS dashboard, FUSE mount, mobile app, or compatibility layer for
another backup format.

Homepage: [fileferry.app](https://fileferry.app/)

Repository: [github.com/dunamismax/fileferry](https://github.com/dunamismax/fileferry)

## Status

`1.0.0-rc.1` is a release candidate, not final v1.0.0.

The current release candidate includes:

- Encrypted local and S3-compatible repository initialization.
- Encrypted, compressed, deduplicated backup snapshots.
- Restore by latest snapshot, snapshot id, tag, and path.
- `snapshots`, `ls`, `check`, `forget`, `prune`, and
  key-management commands.
- JSON and JSONL machine output for the implemented command surface.
- Config profiles and environment-variable precedence.
- Format v0 repository compatibility fixtures for the documented current
  object families.
- Signed release-candidate artifacts, checksums, SBOMs, cargo-auditable
  metadata, installer scripts, and archive-smoke evidence for the intended RC
  targets listed below.

Release-candidate artifacts are intended for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-pc-windows-msvc`

These targets have CI and release-artifact evidence for the RC. Do not read
that as a broad promise that every filesystem feature, metadata field, storage
provider, or operating-system edge case is fully supported. Known limits are
documented below and in [BUILD.md](BUILD.md).

## Install

Download the archive for your target from the
[FileFerry 1.0.0-rc.1 GitHub release](https://github.com/dunamismax/fileferry/releases/tag/v1.0.0-rc.1).

For each target, the GitHub release includes:

- `fileferry-1.0.0-rc.1-<target>.tar.gz`
- `fileferry-1.0.0-rc.1-<target>.SHA256SUMS`
- `fileferry-1.0.0-rc.1-<target>.SHA256SUMS.sigstore.json`
- `fileferry-1.0.0-rc.1-<target>.manifest.json`
- `fileferry-1.0.0-rc.1-<target>.cdx.json`
- `fileferry-<target>.archive-smoke.json`

The release also includes shared `install.sh` and `install.ps1` installer
scripts.

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
beside the archive. They also support explicit checksum input and dry runs.
Because the GitHub release uses target-named checksum assets, pass the matching
checksum file explicitly or save it as `SHA256SUMS` beside the archive.

Manual install:

```sh
tar -xzf fileferry-1.0.0-rc.1-x86_64-unknown-linux-gnu.tar.gz
install -m 0755 fileferry-1.0.0-rc.1-x86_64-unknown-linux-gnu/ferry "$HOME/.local/bin/ferry"
ferry version
```

## Basic Usage

Set a repository URL and passphrase through flags, config, or environment. The
examples below use environment variables for clarity:

```sh
export FILEFERRY_REPOSITORY="$HOME/backups/fileferry-repo"
export FILEFERRY_PASSWORD="change-this-passphrase"

ferry init
ferry backup ~/Documents --tag laptop --jsonl
ferry snapshots --json
ferry find --all --glob 'Projects/**/*.rs'
ferry diff --from-tag laptop --to-latest --path Projects
ferry repo --verify --json
ferry restore latest ~/restore-test
ferry check --read-data-subset 5%
ferry policy set --keep-daily 14 --keep-weekly 8
ferry policy show --json
ferry forget --keep-daily 14 --keep-weekly 8 --dry-run
ferry prune --dry-run
```

S3-compatible repositories use S3-style repository URLs and the documented S3
environment/config path. See [docs/storage.md](docs/storage.md) and
[docs/operations.md](docs/operations.md).

## Scripting Contract

FileFerry is automation-first:

- Stdout is data.
- Stderr is logs, progress, and diagnostics.
- `--json` emits one JSON document on stdout.
- `--jsonl` emits newline-delimited JSON events on stdout.
- Progress bars and spinners do not appear in JSON or JSONL output.
- Destructive commands support `--dry-run`.
- Exit-code families are documented in
  [docs/cli-contract.md](docs/cli-contract.md).

## Security Model

FileFerry encrypts repository data client-side before writing it to local or
object storage.

The current design protects file contents, file names, directory structure,
snapshot metadata, indexes, and sensitive repository config inside
authenticated encrypted objects. Non-sensitive bootstrap fields, such as
format version and KDF parameters, may be plaintext and are documented in
[docs/security.md](docs/security.md) and
[docs/repository-format.md](docs/repository-format.md).

Current primitives and behavior include Argon2id passphrase derivation, a
random repository master key, HKDF-derived subkeys, XChaCha20-Poly1305
authenticated encryption, wrong-key and tamper failure paths, secret-redaction
tests, and encrypted recovery export.

This is release-candidate security engineering, not an external audit claim.

## Known Limits

- `1.0.0-rc.1` is not final v1.0.0.
- Restore applies the implemented metadata subset only. It currently restores
  regular-file and directory modified timestamps, restores Unix mode bits for
  regular files and directories where representable, and verifies Unix
  ownership without calling `chown`.
- xattr values, ACL contents, file flags, resource forks, Windows attributes,
  sparse extent maps, symlink metadata, and creation/birth timestamps are not
  restored by this version.
- Key rotation rotates unlock access by adding/removing key slots. It does not
  rewrite repository data with a new master key.
- The published `1.0.0-rc.1` artifacts include recovery export but not
  recovery import or `ferry find`. Current main adds recovery import as a new
  external key slot, `ferry find` for encrypted snapshot-metadata search,
  `ferry diff` for manifest-level snapshot comparison, `ferry repo` for safe
  repository status plus opt-in encrypted metadata/state verification, and
  `ferry policy` for encrypted repository-local retention policy config
  storage, display, and explicit deletion; full repository rekey is not
  implemented.
- `ferry find` searches decrypted snapshot metadata after repository unlock. It
  does not search file contents or read chunk data.
- `ferry diff` compares decrypted snapshot manifests after repository unlock.
  It reports added, removed, changed, and unchanged entries, including regular
  file content-identity changes from manifest chunk references, but it does not
  read chunk data or compare file contents byte-by-byte.
- `ferry repo` default output avoids decrypted snapshot contents, paths, tags,
  and object keys. `ferry repo --verify` unlocks the repository and verifies
  encrypted metadata, indexes, forget markers, leases, policy, upload, and
  prune state without reading chunk data.
- `ferry policy` stores retention policy config in encrypted repository
  objects. It does not automatically apply stored policies to `forget` yet.
- S3-compatible behavior is tested against the current abstraction and a
  private Backblaze B2 development bucket. It is not a blanket claim for every
  S3-compatible provider.

## Architecture

```text
crates/
  fileferry-cli/       clap commands, config, human output, JSON/JSONL
  fileferry-core/      snapshots, manifests, repository format, engines
  fileferry-storage/   local and S3-compatible object storage
  fileferry-crypto/    KDF, envelope encryption, authenticated objects
  fileferry-platform/  filesystem paths and metadata
  fileferry-policy/    retention and lifecycle policy
  fileferry-testkit/   fake stores and integration-test helpers
  fileferry-web/       public fileferry.app homepage
xtask/                 release packaging and artifact verification
docs/                  security, format, CLI, storage, operations, release
```

The homepage crate is marketing infrastructure for `fileferry.app`; it is not
a FileFerry backup server mode.

## Development

Normal local gate:

```sh
just fmt
just check
just test
just build
```

Equivalent expanded commands:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace
```

Run the homepage locally:

```sh
FILEFERRY_WEB_ADDR=127.0.0.1:8080 cargo run -p fileferry-web
```

Release process and artifact expectations live in
[docs/release.md](docs/release.md). Current release notes live in
[docs/release-notes-1.0.0-rc.1.md](docs/release-notes-1.0.0-rc.1.md).

## License

MIT. See [LICENSE](LICENSE).
