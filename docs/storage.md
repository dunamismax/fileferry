# Storage

FileFerry storage is object-oriented. Backends store immutable byte objects by
validated repository object keys; higher layers decide what those bytes mean.

This document describes the current storage contract. It is not the complete
v1 storage design yet, and it does not claim release support, platform
support, or live S3 prune provider evidence beyond the evidence stated here.

## Object Keys

Object keys are repository-internal names such as `chunks/aa/blob` or
`indexes/current`.

Valid keys:

- Are relative.
- Use `/` as the only separator.
- Do not contain empty, `.`, or `..` segments.
- Do not contain platform path separators such as `\`.
- Use only ASCII letters, digits, `.`, `_`, `-`, and `=` in each segment.

The key validator prevents local backend path traversal and keeps backend
behavior independent from operating-system path syntax. It does not make a
repository object name non-sensitive by itself; repository-format code must
still avoid deriving object names from source paths or backup shape.

## Store Contract

The `ObjectStore` trait currently exposes:

- `capabilities`
- `put_if_absent`
- `get`
- `exists`
- `delete`
- `list_prefix`

`put_if_absent` is the default write primitive for immutable repository
objects. A backend must return `Created` for a new object and `AlreadyPresent`
when the same key already contains identical bytes. If a key exists with
different bytes, the backend must return `ObjectAlreadyExists`.

Deletes are idempotent for the implemented local and fake stores. Deleting a
missing object succeeds.

## Capability Model

`StorageCapabilities` records backend behavior that repository code must not
guess:

- Backend kind.
- Conditional create support.
- Atomic visibility.
- Strong read-after-write behavior.
- Delete behavior.
- Prefix listing support.

The model intentionally separates capability reporting from command output.
CLI code can later map capability failures into stable diagnostics and exit
codes.

`verify_repository_storage_capabilities` is the current destructive capability
probe for a candidate repository store. It first rejects stores that do not
report idempotent delete and prefix listing, then writes a unique object under
the repository-internal `capability-probe/` prefix, reads the exact bytes back,
verifies prefix listing returns that object, deletes it twice, and verifies it
is no longer visible. The probe deletes only its generated object key. It is
evidence for the exact store configuration being tested, not a broad provider
support claim.

## Reliability Policy

`PolicyObjectStore` wraps any `ObjectStore` with the common operational policy
that repository code needs before trusting a backend for long operations:

- Maximum attempts for retryable failures.
- Per-operation timeout.
- Exponential backoff with a configured cap.
- Maximum concurrent object operations.

The default policy is four attempts, a 60-second operation timeout, 100 ms
initial backoff, 2-second maximum backoff, and 16 concurrent operations.

The policy retries transient I/O, backend, and timeout errors. It does not
retry permanent storage-contract failures such as invalid object keys, missing
objects, immutable-write conflicts, permission denial, authentication failure,
unsupported backend operations, or invalid backend paths. Backends still report
their native capabilities; the policy wrapper changes execution behavior, not
backend identity.

## Local Filesystem Backend

`LocalStore` maps validated object keys under a configured root directory.

Writes use this flow:

1. Create parent directories for the final object path.
2. Write bytes to a unique file under `.fileferry-tmp/`.
3. Sync the temporary file.
4. Publish by hard-linking the temporary file to the final object path.
5. Remove the temporary file.

If the final object already exists, the local backend removes the temporary
file and compares existing bytes. Identical bytes make the operation
idempotent; different bytes fail as an immutable write conflict.

Leftover `.fileferry-tmp/` files are ignored by prefix listing so interrupted
writes do not appear as repository objects.

## Fake Store

`fileferry-testkit` provides `FakeObjectStore`, an in-memory implementation of
the same object-store contract. It is for repository, corruption, and pipeline
tests that need deterministic storage behavior without touching a real backend.

The fake store enforces the same immutable write rule as the local backend:
same bytes are idempotent, different bytes are rejected.

## S3-Compatible Backend

`S3Store` is the first S3-compatible backend adapter. It uses the Rust
`object_store` crate's AWS/S3 implementation, configured with an explicit
bucket, region, endpoint URL, access key ID, secret access key, and repository
root prefix.

The backend maps validated FileFerry object keys under the configured root
prefix. For example, a repository key `chunks/aa/blob` with root prefix
`fileferry/dev` is stored at `fileferry/dev/chunks/aa/blob` in the bucket.
Prefix-scoping is mandatory for shared development buckets so tests never need
to list or delete the whole bucket.

`put_if_absent` uses S3 conditional create semantics through
`PutMode::Create` when the provider supports `If-None-Match` on `PutObject`.
When conditional create is disabled for a provider, the backend falls back to a
head/read-before-write flow and reports `conditional_create = false` in
capabilities. That fallback preserves idempotent same-byte retries, but it is
not race-safe for concurrent writers.

S3 credentials are accepted as secret values and redacted from debug output.
They must come from local environment, secret stores, or future config-secret
plumbing. Do not commit credentials, signed URLs, repository URLs containing
secrets, or local `.env` files.

Current S3 capability assumptions:

- HTTPS endpoint required.
- Path-style requests are used.
- Conditional create is provider-dependent and reported in capabilities.
- Deletes are treated as idempotent.
- Prefix listing is available.
- Object tags are disabled because some S3-compatible providers reject tagging
  headers.

Current storage-layer S3 evidence includes:

- A reusable repository capability probe that rejects missing idempotent-delete
  or prefix-listing capabilities before writing, then verifies write, immediate
  read, prefix listing, idempotent delete, and cleanup for the tested store.
- Retry policy tests for retryable put, read, delete, and list failures.
- A permanent permission-denied backend failure test that verifies the policy
  does not retry access failures.
- Prefix-mapping tests that ignore exact root markers and sibling prefixes
  outside the configured repository root.
- A fail-closed test for invalid object keys returned from backend listing.

The implementation has a gated live integration test. It runs only when
`FILEFERRY_S3_INTEGRATION=1` and all required S3 environment variables are set:

```sh
export FILEFERRY_S3_INTEGRATION=1
export FILEFERRY_S3_BUCKET=dunamismax-b2
export FILEFERRY_S3_REGION=<region>
export FILEFERRY_S3_ENDPOINT=https://s3.<region>.backblazeb2.com
export FILEFERRY_S3_ACCESS_KEY_ID=<application-key-id>
export FILEFERRY_S3_SECRET_ACCESS_KEY=<application-key>
export FILEFERRY_S3_TEST_PREFIX=fileferry/dev

cargo test -p fileferry-storage s3_store_round_trip_when_integration_env_is_enabled
```

That gated test runs the capability probe before the round trip and uses a
unique suffix under `FILEFERRY_S3_TEST_PREFIX`.

For Backblaze B2, the S3 endpoint has the form
`https://s3.<region>.backblazeb2.com`, and the region is the second component
of the endpoint, such as `us-west-001`. The current Backblaze test disables
conditional create because Backblaze returns `501 NotImplemented` for the
`If-None-Match` create-only request header used by `object_store`.

## S3-Compatible CLI Commands

`ferry init`, `ferry backup`, `ferry snapshots`, `ferry ls`, `ferry find`,
`ferry diff`, `ferry repo`, `ferry restore`, `ferry check`, `ferry forget`,
`ferry prune`, and `ferry key ...` commands accept S3-compatible repository
URLs in this form:

```sh
FILEFERRY_PASSWORD='test-passphrase' \
FILEFERRY_S3_ENDPOINT='https://s3.us-west-001.backblazeb2.com' \
FILEFERRY_S3_REGION='us-west-001' \
FILEFERRY_S3_ACCESS_KEY_ID='<application-key-id>' \
FILEFERRY_S3_SECRET_ACCESS_KEY='<application-key>' \
ferry --repo 's3://dunamismax-b2/fileferry/dev/example-repo' init
```

The URL supplies the bucket (`dunamismax-b2`) and repository root prefix
(`fileferry/dev/example-repo`). Credentials are supplied only through
environment variables and must not be embedded in repository URLs. Query
strings and fragments are rejected. Human, JSON, JSONL, and error output
redacts S3 repository URLs as `s3://<redacted>` and does not emit S3
credentials.

Set `FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE=1` for the current Backblaze B2
development path because Backblaze rejects the create-only `PutObject` header
used by the default conditional-create mode. That fallback is idempotent for
same-byte retries but is not race-safe for concurrent writers.

The CLI has separate gated live init and data-path tests. The init test runs
only when
`FILEFERRY_S3_INIT_INTEGRATION=1` and the same S3 environment variables plus
`FILEFERRY_S3_TEST_PREFIX` are set. The test appends a unique
`cli-init-...` suffix under `FILEFERRY_S3_TEST_PREFIX`, initializes only that
repository prefix, and deletes the `bootstrap` object it creates.

The data-path test runs only when `FILEFERRY_S3_DATA_INTEGRATION=1` and the
same S3 environment variables plus `FILEFERRY_S3_TEST_PREFIX` are set. It
appends a unique `cli-data-...` suffix, initializes that repository prefix,
runs backup, snapshots, ls, restore, and check through the `ferry` binary,
deletes a referenced manifest to verify missing-object failure behavior, and
then deletes objects under only that unique repository prefix.

The retention/key-management test runs only when
`FILEFERRY_S3_RETENTION_KEY_INTEGRATION=1` and the same S3 environment
variables plus `FILEFERRY_S3_TEST_PREFIX` are set. It appends a unique
`cli-retention-key-...` suffix, initializes that repository prefix, runs
backup, forget, snapshots, key add, key remove, key rotate, and
key export-recovery, and key import-recovery through the `ferry` binary,
verifies removed key slots no longer unlock the repository, verifies the
imported recovery key slot can unlock the repository, and then deletes objects
under only that unique repository prefix. The recovery export is written to a
local temporary file; it is not stored in S3.

The S3 prune test runs only when `FILEFERRY_S3_PRUNE_INTEGRATION=1` and the
same S3 environment variables plus `FILEFERRY_S3_TEST_PREFIX` are set. It
appends a unique `cli-prune-...` suffix, initializes that repository prefix,
runs backup, forget, prune dry-run, prune sweep, and snapshots through the
`ferry` binary, verifies prune plan/completion objects exist, and deletes
objects under only that unique repository prefix.

The Backblaze B2 live drill on 2026-05-22 passed these S3-compatible storage
and CLI gates under an isolated private development prefix. That is current
Backblaze provider evidence only; it is not a broad S3 provider support claim.

S3 prune deletes existing repository objects and writes small encrypted
prune-state objects through `put_if_absent`; it does not initiate multipart
uploads. Multipart or partial-upload cleanup remains part of the broader S3
backup/upload hardening path, not prune itself.

## Not Implemented Yet

S3-compatible storage now has the initial object-store adapter and live
Backblaze round-trip, data-path, retention/key-management, and prune test
gates.
`ferry init`, `ferry backup`, `ferry snapshots`, `ferry ls`, `ferry find`,
`ferry diff`, `ferry repo`, `ferry restore`, `ferry check`, `ferry forget`,
`ferry prune`, and `ferry key ...` commands use S3-compatible encrypted
repositories through the same core repository pipeline as the local backend.
Common retry, timeout, backoff, and concurrency behavior exists in the policy
wrapper, and a basic destructive capability probe now exists for exact store
configurations. Before broader S3 provider support is claimed, FileFerry still
needs live evidence for providers beyond the current Backblaze B2 path,
live-S3 `ferry find`, `ferry diff`, and `ferry repo` drill coverage, partial
upload behavior, and multipart cleanup guidance.
