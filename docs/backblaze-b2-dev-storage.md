# Backblaze B2 Development Storage

FileFerry has a private Backblaze B2 development bucket for live S3-compatible
storage testing:

```text
Bucket name: dunamismax-b2
Bucket type: private
```

Credentials are intentionally not stored in this repository. Keep Backblaze
application key IDs and application keys in your local shell, password manager,
or future secret-provider plumbing only. Local `.env` files are ignored by git.

## Environment

The S3 integration test in `fileferry-storage` is opt-in. Export these values
locally before running it:

```sh
export FILEFERRY_S3_INTEGRATION=1
export FILEFERRY_S3_BUCKET=dunamismax-b2
export FILEFERRY_S3_REGION=<region>
export FILEFERRY_S3_ENDPOINT=https://s3.<region>.backblazeb2.com
export FILEFERRY_S3_ACCESS_KEY_ID=<application-key-id>
export FILEFERRY_S3_SECRET_ACCESS_KEY=<application-key>
export FILEFERRY_S3_TEST_PREFIX=fileferry/dev
```

Do not include leading or trailing slashes in `FILEFERRY_S3_TEST_PREFIX`.
The test appends a unique `run-...` suffix below that prefix, writes only test
objects inside it, and cleans up the objects it creates.

Run the live test with:

```sh
cargo test -p fileferry-storage s3_store_round_trip_when_integration_env_is_enabled
```

or:

```sh
just test-s3
```

The normal workspace test suite does not contact Backblaze unless
`FILEFERRY_S3_INTEGRATION=1` is present.

The CLI S3 init integration test uses the same bucket, endpoint, region,
credential, and test-prefix variables, plus a separate opt-in gate:

```sh
export FILEFERRY_S3_INIT_INTEGRATION=1
cargo test -p fileferry-cli init_s3_live_integration_when_env_is_enabled
```

The CLI test creates a unique `cli-init-...` repository prefix under
`FILEFERRY_S3_TEST_PREFIX`, runs `ferry init`, verifies the JSON output is
redacted, and deletes only the `bootstrap` object it created.

The CLI S3 data-path integration test uses the same variables, plus its own
opt-in gate:

```sh
export FILEFERRY_S3_DATA_INTEGRATION=1
cargo test -p fileferry-cli s3_data_path_live_integration_when_env_is_enabled
```

The data-path test creates a unique `cli-data-...` repository prefix under
`FILEFERRY_S3_TEST_PREFIX`, runs `ferry init`, `backup`, `snapshots`, `ls`,
`restore`, and `check`, verifies a missing referenced manifest fails closed,
and deletes only objects under that unique repository prefix.

The CLI S3 retention/key-management integration test uses the same variables,
plus its own opt-in gate:

```sh
export FILEFERRY_S3_RETENTION_KEY_INTEGRATION=1
cargo test -p fileferry-cli s3_retention_key_management_live_integration_when_env_is_enabled
```

The retention/key-management test creates a unique `cli-retention-key-...`
repository prefix under `FILEFERRY_S3_TEST_PREFIX`, runs `ferry init`,
`backup`, `forget`, `snapshots`, `key add`, `key remove`, `key rotate`, and
`key export-recovery`, and `key import-recovery`, verifies removed key slots no
longer unlock the repository, verifies the imported recovery key slot can
unlock the repository, writes the recovery export only to a local temporary
file, and deletes only objects under that unique repository prefix.

The CLI S3 prune integration test uses the same variables, plus its own opt-in
gate:

```sh
export FILEFERRY_S3_PRUNE_INTEGRATION=1
cargo test -p fileferry-cli s3_prune_live_integration_when_env_is_enabled
```

The prune test creates a unique `cli-prune-...` repository prefix under
`FILEFERRY_S3_TEST_PREFIX`, runs `ferry init`, `backup`, `forget`,
`prune --dry-run`, `prune`, and `snapshots`, verifies encrypted prune
plan/completion state exists, and deletes only objects under that unique
repository prefix.

The CLI S3 command-surface integration test uses the same variables, plus its
own opt-in gate:

```sh
export FILEFERRY_S3_COMMAND_SURFACE_INTEGRATION=1
export FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE=1
cargo test -p fileferry-cli s3_command_surface_live_integration_when_env_is_enabled
```

The command-surface test creates a unique `cli-command-surface-...` repository
prefix under `FILEFERRY_S3_TEST_PREFIX`, runs `ferry init`, two `backup`
commands, `find`, `diff`, `repo --verify`, `doctor --jsonl`, `policy set`,
`policy show`, and `key rekey --jsonl`, verifies old unlock material no longer
opens the repository after rekey, verifies the new passphrase can read
snapshots and encrypted policy config, and deletes only objects under that
unique repository prefix.

## Backblaze S3 Notes

Backblaze documents S3-compatible endpoints as:

```text
https://s3.<region>.backblazeb2.com
```

The region is the second component of the endpoint. For example,
`https://s3.us-west-001.backblazeb2.com` uses region `us-west-001`.

Backblaze S3-compatible authentication uses:

- Application Key ID as the S3 access key ID.
- Application Key as the S3 secret access key.
- Signature v4.
- HTTPS endpoints.

The current live test disables S3 conditional create for Backblaze B2 because
Backblaze rejects the `If-None-Match` create-only `PutObject` header with
`501 NotImplemented`. FileFerry reports that weaker capability instead of
pretending the backend has race-safe conditional writes.

For manual Backblaze CLI init tests, set
`FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE=1` with the normal S3 environment:

```sh
FILEFERRY_PASSWORD='throwaway-passphrase' \
FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE=1 \
ferry --repo "s3://dunamismax-b2/${FILEFERRY_S3_TEST_PREFIX}/manual-init" init
```

Use only throwaway prefixes under `FILEFERRY_S3_TEST_PREFIX`. Do not run
manual tests against production repositories or real user backup data.

Bucket-restricted application keys may need `listAllBucketNames` for some SDKs
or integrations. FileFerry's live test should still be scoped by
`FILEFERRY_S3_TEST_PREFIX` and must never operate on unrelated bucket objects.

Primary references:

- <https://help.backblaze.com/hc/en-us/articles/360047425453-Getting-Started-with-the-S3-Compatible-API>
- <https://www.backblaze.com/docs/cloud-storage-call-the-s3-compatible-api>
