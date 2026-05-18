# Backblaze B2 Development Storage

SealPort has a private Backblaze B2 development bucket for live S3-compatible
storage testing:

```text
Bucket name: dunamismax-b2
Bucket type: private
```

Credentials are intentionally not stored in this repository. Keep Backblaze
application key IDs and application keys in your local shell, password manager,
or future secret-provider plumbing only. Local `.env` files are ignored by git.

## Environment

The S3 integration test in `sealport-storage` is opt-in. Export these values
locally before running it:

```sh
export SEALPORT_S3_INTEGRATION=1
export SEALPORT_S3_BUCKET=dunamismax-b2
export SEALPORT_S3_REGION=<region>
export SEALPORT_S3_ENDPOINT=https://s3.<region>.backblazeb2.com
export SEALPORT_S3_ACCESS_KEY_ID=<application-key-id>
export SEALPORT_S3_SECRET_ACCESS_KEY=<application-key>
export SEALPORT_S3_TEST_PREFIX=sealport/dev
```

Do not include leading or trailing slashes in `SEALPORT_S3_TEST_PREFIX`.
The test appends a unique `run-...` suffix below that prefix, writes only test
objects inside it, and cleans up the objects it creates.

Run the live test with:

```sh
cargo test -p sealport-storage s3_store_round_trip_when_integration_env_is_enabled
```

or:

```sh
just test-s3
```

The normal workspace test suite does not contact Backblaze unless
`SEALPORT_S3_INTEGRATION=1` is present.

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

Bucket-restricted application keys may need `listAllBucketNames` for some SDKs
or integrations. SealPort's live test should still be scoped by
`SEALPORT_S3_TEST_PREFIX` and must never operate on unrelated bucket objects.

Primary references:

- <https://help.backblaze.com/hc/en-us/articles/360047425453-Getting-Started-with-the-S3-Compatible-API>
- <https://www.backblaze.com/docs/cloud-storage-call-the-s3-compatible-api>
