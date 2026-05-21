use fileferry_core::{
    BackupPipeline, BackupPipelineConfig, CheckRepositoryOptions, CoreError, PruneObjectKind,
    PruneRepositoryOptions, RepositoryAeadAlgorithm, RepositoryFormatCompatibility,
    RepositoryLeaseCommandKind, RepositoryLeaseStateRequest, RepositoryPolicyConfigRequest,
    RepositoryUploadOperation, RepositoryUploadPendingObject, RepositoryUploadPendingObjectKind,
    RepositoryUploadStateRequest, RestoreContentRequest, SnapshotManifest, create_repository,
    inspect_repository_format, open_repository, verify_repository_recovery_export,
};
use fileferry_crypto::{
    AeadAlgorithm, EncryptedObject, KeyPurpose, ObjectContext, ObjectKind, decrypt_object,
    encrypt_object,
};
use fileferry_storage::{ObjectKey, ObjectStore};
use fileferry_testkit::FakeObjectStore;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};

const REPOSITORY_ID: &str = "b65c7dfa2394e1b21ebea003397da66b721cc793b376e1edcd78d7f990954771";
const KEY_SLOT_ID: &str = "370e4852331603403cbd038dfa1f4cc4577d2f326f94a19a218be7dc88921f50";
const PRIMARY_PASSPHRASE: &str = "fixture-primary-passphrase-v0";
const ADDED_PASSPHRASE: &str = "fixture-added-passphrase-v0";
const SNAPSHOT_DATA_REPOSITORY_ID: &str =
    "ea3bea77f6468b546f7b486a4f7d498e984b718d22ba9bc7ca2e09bf6b013794";
const SNAPSHOT_DATA_PASSPHRASE: &str = "snapshot-data-fixture-passphrase-v0";
const SNAPSHOT_ID: &str = "23502e57ab2eb5ffad9bbe1361cd2d2687d9d167366b4eb01218f471db276b33";
const INDEX_ID: &str = "6fb18d148cffd5bf241577aafe8178be33643c4b13e023e7a89b8c2af5240bcd";
const MANIFEST_OBJECT: &str =
    "objects/manifest/23/23502e57ab2eb5ffad9bbe1361cd2d2687d9d167366b4eb01218f471db276b33";
const INDEX_OBJECT: &str =
    "objects/index/6f/6fb18d148cffd5bf241577aafe8178be33643c4b13e023e7a89b8c2af5240bcd";
const FIRST_CHUNK_OBJECT: &str =
    "objects/chunk/02/025080c9b7fb31b68ac19a42d8685341c4720d8e8e54b51a73203ec37cdfb6c6";
const SECOND_CHUNK_OBJECT: &str =
    "objects/chunk/11/1158295b80156c95e5f834e39001dbe1f2be572c94e52314aef27ab9af50cae3";
const COMMIT_OBJECT: &str =
    "commits/23502e57ab2eb5ffad9bbe1361cd2d2687d9d167366b4eb01218f471db276b33";
const FORGET_PRUNE_REPOSITORY_ID: &str =
    "80fb1310ac9c3293a713888c4f470aaf3e02d5518a852639ec6c32afd0a83749";
const FORGET_PRUNE_PASSPHRASE: &str = "forget-prune-fixture-passphrase-v0";
const FORGOTTEN_SNAPSHOT_ID: &str =
    "80a86bb83a513f56ae8c36263af3438170cd309777e3397cb2d1c8049e56bdb6";
const RETAINED_SNAPSHOT_ID: &str =
    "fa502652337a0db2c09fbe5d6916c0dd2920932691f63be8be9317b23548b6da";
const PRUNE_PLAN_ID: &str = "e0ab98bab38ae3654bf1a54813126d192fa5235797cf16942c692683cc01cec1";
const FORGET_MARKER_OBJECT: &str =
    "forgets/80a86bb83a513f56ae8c36263af3438170cd309777e3397cb2d1c8049e56bdb6";
const PRUNE_PLAN_OBJECT: &str =
    "objects/prune-plan/e0/e0ab98bab38ae3654bf1a54813126d192fa5235797cf16942c692683cc01cec1";
const PRUNE_COMPLETION_OBJECT: &str =
    "objects/prune-completion/e0/e0ab98bab38ae3654bf1a54813126d192fa5235797cf16942c692683cc01cec1";
const RETAINED_COMMIT_OBJECT: &str =
    "commits/fa502652337a0db2c09fbe5d6916c0dd2920932691f63be8be9317b23548b6da";
const RETAINED_MANIFEST_OBJECT: &str =
    "objects/manifest/fa/fa502652337a0db2c09fbe5d6916c0dd2920932691f63be8be9317b23548b6da";
const RETAINED_INDEX_OBJECT: &str =
    "objects/index/17/173970a92ca87e28601637550571fdd7dc42d7d673c2a3b00709bc99ec0c8e10";
const RETAINED_CHUNK_OBJECT: &str =
    "objects/chunk/33/3357bb277f6fa50b3762efb6284930137bdd5fb5e01cea053ce44f412d313c79";
const POLICY_CONFIG_REPOSITORY_ID: &str =
    "8a45630e2ea7ccfa88914e9489c83954741de9e31d0bdd41de3a6a6b92c476f6";
const POLICY_CONFIG_PASSPHRASE: &str = "policy-config-fixture-passphrase-v0";
const POLICY_ID: &str = "382b7e84bd6ac92b93aba74dd9c2733fd97f68f479055a20383b19744622e6f8";
const POLICY_OBJECT: &str =
    "objects/policy/38/382b7e84bd6ac92b93aba74dd9c2733fd97f68f479055a20383b19744622e6f8";
const UPLOAD_STATE_REPOSITORY_ID: &str =
    "c152dc99c29a879eed76014cb583b17fd993b8208ad0c8590af34d4571df5d91";
const UPLOAD_STATE_PASSPHRASE: &str = "upload-state-fixture-passphrase-v0";
const UPLOAD_WRITER_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const UPLOAD_ID: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const UPLOAD_STATE_OBJECT: &str = "objects/upload/1111111111111111111111111111111111111111111111111111111111111111/2222222222222222222222222222222222222222222222222222222222222222";
const LEASE_STATE_REPOSITORY_ID: &str =
    "10c715af19853fec72fde8f49904ab36abcd358967f084720d2f1722d201b46b";
const LEASE_STATE_PASSPHRASE: &str = "lease-state-fixture-passphrase-v0";
const LEASE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const LEASE_WRITER_ID: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const LEASE_STATE_OBJECT: &str =
    "locks/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const BOOTSTRAP: &[u8] =
    include_bytes!("../../../tests/fixtures/repository-format/v0/bootstrap-keyslot/bootstrap");
const KEY_SLOT: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/bootstrap-keyslot/key-slots/370e4852331603403cbd038dfa1f4cc4577d2f326f94a19a218be7dc88921f50"
);
const KEY_SLOT_REMOVAL: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/bootstrap-keyslot/key-slot-removals/370e4852331603403cbd038dfa1f4cc4577d2f326f94a19a218be7dc88921f50"
);
const RECOVERY_EXPORT: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/recovery-export/recovery.fileferry-key"
);
const SNAPSHOT_DATA_BOOTSTRAP: &[u8] =
    include_bytes!("../../../tests/fixtures/repository-format/v0/snapshot-data/bootstrap");
const SNAPSHOT_DATA_COMMIT: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/snapshot-data/commits/23502e57ab2eb5ffad9bbe1361cd2d2687d9d167366b4eb01218f471db276b33"
);
const SNAPSHOT_DATA_MANIFEST: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/snapshot-data/objects/manifest/23/23502e57ab2eb5ffad9bbe1361cd2d2687d9d167366b4eb01218f471db276b33"
);
const SNAPSHOT_DATA_INDEX: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/snapshot-data/objects/index/6f/6fb18d148cffd5bf241577aafe8178be33643c4b13e023e7a89b8c2af5240bcd"
);
const SNAPSHOT_DATA_FIRST_CHUNK: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/snapshot-data/objects/chunk/02/025080c9b7fb31b68ac19a42d8685341c4720d8e8e54b51a73203ec37cdfb6c6"
);
const SNAPSHOT_DATA_SECOND_CHUNK: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/snapshot-data/objects/chunk/11/1158295b80156c95e5f834e39001dbe1f2be572c94e52314aef27ab9af50cae3"
);
const FORGET_PRUNE_BOOTSTRAP: &[u8] =
    include_bytes!("../../../tests/fixtures/repository-format/v0/forget-prune-state/bootstrap");
const FORGET_PRUNE_FORGET_MARKER: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/forget-prune-state/forgets/80a86bb83a513f56ae8c36263af3438170cd309777e3397cb2d1c8049e56bdb6"
);
const FORGET_PRUNE_RETAINED_COMMIT: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/forget-prune-state/commits/fa502652337a0db2c09fbe5d6916c0dd2920932691f63be8be9317b23548b6da"
);
const FORGET_PRUNE_RETAINED_MANIFEST: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/forget-prune-state/objects/manifest/fa/fa502652337a0db2c09fbe5d6916c0dd2920932691f63be8be9317b23548b6da"
);
const FORGET_PRUNE_RETAINED_INDEX: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/forget-prune-state/objects/index/17/173970a92ca87e28601637550571fdd7dc42d7d673c2a3b00709bc99ec0c8e10"
);
const FORGET_PRUNE_RETAINED_CHUNK: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/forget-prune-state/objects/chunk/33/3357bb277f6fa50b3762efb6284930137bdd5fb5e01cea053ce44f412d313c79"
);
const FORGET_PRUNE_PLAN: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/forget-prune-state/objects/prune-plan/e0/e0ab98bab38ae3654bf1a54813126d192fa5235797cf16942c692683cc01cec1"
);
const FORGET_PRUNE_COMPLETION: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/forget-prune-state/objects/prune-completion/e0/e0ab98bab38ae3654bf1a54813126d192fa5235797cf16942c692683cc01cec1"
);
const POLICY_CONFIG_BOOTSTRAP: &[u8] =
    include_bytes!("../../../tests/fixtures/repository-format/v0/policy-config/bootstrap");
const POLICY_CONFIG: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/policy-config/objects/policy/38/382b7e84bd6ac92b93aba74dd9c2733fd97f68f479055a20383b19744622e6f8"
);
const UPLOAD_STATE_BOOTSTRAP: &[u8] =
    include_bytes!("../../../tests/fixtures/repository-format/v0/upload-state/bootstrap");
const UPLOAD_STATE: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/upload-state/objects/upload/1111111111111111111111111111111111111111111111111111111111111111/2222222222222222222222222222222222222222222222222222222222222222"
);
const LEASE_STATE_BOOTSTRAP: &[u8] =
    include_bytes!("../../../tests/fixtures/repository-format/v0/lease-state/bootstrap");
const LEASE_STATE: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/lease-state/locks/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
);
const MIGRATION_FUTURE_FORMAT_BOOTSTRAP: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/migration/future-format-bootstrap/bootstrap"
);
const MIGRATION_FUTURE_FEATURE_BOOTSTRAP: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/migration/future-feature-bootstrap/bootstrap"
);
const MIGRATION_UNVERSIONED_BOOTSTRAP: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/migration/unversioned-bootstrap/bootstrap"
);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct FixtureEncryptedObject {
    algorithm: RepositoryAeadAlgorithm,
    nonce: [u8; fileferry_crypto::XCHACHA20_POLY1305_NONCE_LEN],
    ciphertext: Vec<u8>,
}

fn object_key(value: &str) -> ObjectKey {
    ObjectKey::new(value).expect("valid object key")
}

fn key_slot_object_key() -> ObjectKey {
    object_key(&format!("key-slots/{KEY_SLOT_ID}"))
}

fn key_slot_removal_object_key() -> ObjectKey {
    object_key(&format!("key-slot-removals/{KEY_SLOT_ID}"))
}

fn secret(value: &str) -> SecretString {
    SecretString::from(value)
}

async fn load_bootstrap_and_key_slot() -> FakeObjectStore {
    let store = FakeObjectStore::new();
    store
        .overwrite_for_tests(object_key("bootstrap"), BOOTSTRAP.to_vec())
        .await;
    store
        .overwrite_for_tests(key_slot_object_key(), KEY_SLOT.to_vec())
        .await;
    store
}

async fn load_complete_fixture() -> FakeObjectStore {
    let store = load_bootstrap_and_key_slot().await;
    store
        .overwrite_for_tests(key_slot_removal_object_key(), KEY_SLOT_REMOVAL.to_vec())
        .await;
    store
}

async fn load_snapshot_data_fixture() -> FakeObjectStore {
    let store = FakeObjectStore::new();
    for (key, bytes) in [
        ("bootstrap", SNAPSHOT_DATA_BOOTSTRAP),
        (COMMIT_OBJECT, SNAPSHOT_DATA_COMMIT),
        (MANIFEST_OBJECT, SNAPSHOT_DATA_MANIFEST),
        (INDEX_OBJECT, SNAPSHOT_DATA_INDEX),
        (FIRST_CHUNK_OBJECT, SNAPSHOT_DATA_FIRST_CHUNK),
        (SECOND_CHUNK_OBJECT, SNAPSHOT_DATA_SECOND_CHUNK),
    ] {
        store
            .overwrite_for_tests(object_key(key), bytes.to_vec())
            .await;
    }
    store
}

async fn load_forget_prune_state_fixture() -> FakeObjectStore {
    let store = FakeObjectStore::new();
    for (key, bytes) in [
        ("bootstrap", FORGET_PRUNE_BOOTSTRAP),
        (FORGET_MARKER_OBJECT, FORGET_PRUNE_FORGET_MARKER),
        (RETAINED_COMMIT_OBJECT, FORGET_PRUNE_RETAINED_COMMIT),
        (RETAINED_MANIFEST_OBJECT, FORGET_PRUNE_RETAINED_MANIFEST),
        (RETAINED_INDEX_OBJECT, FORGET_PRUNE_RETAINED_INDEX),
        (RETAINED_CHUNK_OBJECT, FORGET_PRUNE_RETAINED_CHUNK),
        (PRUNE_PLAN_OBJECT, FORGET_PRUNE_PLAN),
        (PRUNE_COMPLETION_OBJECT, FORGET_PRUNE_COMPLETION),
    ] {
        store
            .overwrite_for_tests(object_key(key), bytes.to_vec())
            .await;
    }
    store
}

async fn load_pending_prune_plan_fixture() -> FakeObjectStore {
    let store = FakeObjectStore::new();
    for (key, bytes) in [
        ("bootstrap", FORGET_PRUNE_BOOTSTRAP),
        (FORGET_MARKER_OBJECT, FORGET_PRUNE_FORGET_MARKER),
        (PRUNE_PLAN_OBJECT, FORGET_PRUNE_PLAN),
    ] {
        store
            .overwrite_for_tests(object_key(key), bytes.to_vec())
            .await;
    }
    store
}

async fn load_policy_config_fixture() -> FakeObjectStore {
    let store = FakeObjectStore::new();
    for (key, bytes) in [
        ("bootstrap", POLICY_CONFIG_BOOTSTRAP),
        (POLICY_OBJECT, POLICY_CONFIG),
    ] {
        store
            .overwrite_for_tests(object_key(key), bytes.to_vec())
            .await;
    }
    store
}

async fn load_upload_state_fixture() -> FakeObjectStore {
    let store = FakeObjectStore::new();
    for (key, bytes) in [
        ("bootstrap", UPLOAD_STATE_BOOTSTRAP),
        (UPLOAD_STATE_OBJECT, UPLOAD_STATE),
    ] {
        store
            .overwrite_for_tests(object_key(key), bytes.to_vec())
            .await;
    }
    store
}

async fn load_lease_state_fixture() -> FakeObjectStore {
    let store = FakeObjectStore::new();
    for (key, bytes) in [
        ("bootstrap", LEASE_STATE_BOOTSTRAP),
        (LEASE_STATE_OBJECT, LEASE_STATE),
    ] {
        store
            .overwrite_for_tests(object_key(key), bytes.to_vec())
            .await;
    }
    store
}

async fn load_migration_bootstrap_fixture(bytes: &[u8]) -> FakeObjectStore {
    let store = FakeObjectStore::new();
    store
        .overwrite_for_tests(object_key("bootstrap"), bytes.to_vec())
        .await;
    store
}

fn snapshot_data_pipeline() -> BackupPipeline {
    BackupPipeline::new(BackupPipelineConfig::new(SNAPSHOT_DATA_REPOSITORY_ID))
        .expect("snapshot-data pipeline")
}

fn forget_prune_pipeline() -> BackupPipeline {
    BackupPipeline::new(BackupPipelineConfig::new(FORGET_PRUNE_REPOSITORY_ID))
        .expect("forget-prune pipeline")
}

fn policy_config_pipeline() -> BackupPipeline {
    BackupPipeline::new(BackupPipelineConfig::new(POLICY_CONFIG_REPOSITORY_ID))
        .expect("policy-config pipeline")
}

fn upload_state_pipeline() -> BackupPipeline {
    BackupPipeline::new(BackupPipelineConfig::new(UPLOAD_STATE_REPOSITORY_ID))
        .expect("upload-state pipeline")
}

fn lease_state_pipeline() -> BackupPipeline {
    BackupPipeline::new(BackupPipelineConfig::new(LEASE_STATE_REPOSITORY_ID))
        .expect("lease-state pipeline")
}

fn decode_fixture_encrypted_object(bytes: &[u8]) -> EncryptedObject {
    let stored: FixtureEncryptedObject =
        serde_json::from_slice(bytes).expect("fixture encrypted object frame");
    let algorithm = match stored.algorithm {
        RepositoryAeadAlgorithm::XChaCha20Poly1305 => AeadAlgorithm::XChaCha20Poly1305,
    };
    EncryptedObject {
        algorithm,
        nonce: stored.nonce,
        ciphertext: stored.ciphertext,
    }
}

fn encode_fixture_encrypted_object(object: EncryptedObject) -> Vec<u8> {
    let algorithm = match object.algorithm {
        AeadAlgorithm::XChaCha20Poly1305 => RepositoryAeadAlgorithm::XChaCha20Poly1305,
    };
    serde_json::to_vec(&FixtureEncryptedObject {
        algorithm,
        nonce: object.nonce,
        ciphertext: object.ciphertext,
    })
    .expect("fixture encrypted object frame")
}

fn tamper_fixture_ciphertext(bytes: &[u8]) -> Vec<u8> {
    let mut object = decode_fixture_encrypted_object(bytes);
    object.ciphertext[0] ^= 0x01;
    encode_fixture_encrypted_object(object)
}

fn reencrypt_snapshot_manifest_variant(
    opened: &fileferry_core::OpenedRepository,
    mutate: impl FnOnce(&mut SnapshotManifest),
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(
            KeyPurpose::SnapshotMetadata,
            SNAPSHOT_DATA_REPOSITORY_ID.as_bytes(),
        )
        .expect("manifest subkey");
    let context = ObjectContext::new(ObjectKind::SnapshotManifest, MANIFEST_OBJECT)
        .expect("manifest context");
    let plaintext = decrypt_object(
        &key,
        &context,
        &decode_fixture_encrypted_object(SNAPSHOT_DATA_MANIFEST),
    )
    .expect("fixture manifest decrypts");
    let mut manifest: SnapshotManifest =
        serde_json::from_slice(&plaintext).expect("fixture manifest json");
    mutate(&mut manifest);
    let mutated = serde_json::to_vec(&manifest).expect("mutated manifest json");
    let encrypted = encrypt_object(&key, &context, &mutated).expect("mutated manifest encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_snapshot_manifest_plaintext(
    opened: &fileferry_core::OpenedRepository,
    plaintext: &[u8],
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(
            KeyPurpose::SnapshotMetadata,
            SNAPSHOT_DATA_REPOSITORY_ID.as_bytes(),
        )
        .expect("manifest subkey");
    let context = ObjectContext::new(ObjectKind::SnapshotManifest, MANIFEST_OBJECT)
        .expect("manifest context");
    let encrypted = encrypt_object(&key, &context, plaintext).expect("manifest plaintext encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_chunk_index_variant(
    opened: &fileferry_core::OpenedRepository,
    mutate: impl FnOnce(&mut fileferry_core::ChunkIndex),
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(KeyPurpose::Index, SNAPSHOT_DATA_REPOSITORY_ID.as_bytes())
        .expect("index subkey");
    let context = ObjectContext::new(ObjectKind::Index, INDEX_OBJECT).expect("index context");
    let plaintext = decrypt_object(
        &key,
        &context,
        &decode_fixture_encrypted_object(SNAPSHOT_DATA_INDEX),
    )
    .expect("fixture index decrypts");
    let mut index: fileferry_core::ChunkIndex =
        serde_json::from_slice(&plaintext).expect("fixture index json");
    mutate(&mut index);
    let mutated = serde_json::to_vec(&index).expect("mutated index json");
    let encrypted = encrypt_object(&key, &context, &mutated).expect("mutated index encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_prune_mark_value(
    opened: &fileferry_core::OpenedRepository,
    object_name: &str,
    bytes: &[u8],
    mutate: impl FnOnce(&mut Value),
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(KeyPurpose::PruneMark, FORGET_PRUNE_REPOSITORY_ID.as_bytes())
        .expect("prune subkey");
    let context = ObjectContext::new(ObjectKind::PruneMark, object_name).expect("prune context");
    let plaintext = decrypt_object(&key, &context, &decode_fixture_encrypted_object(bytes))
        .expect("fixture prune mark decrypts");
    let mut value: Value = serde_json::from_slice(&plaintext).expect("fixture prune mark json");
    mutate(&mut value);
    let mutated = serde_json::to_vec(&value).expect("mutated prune mark json");
    let encrypted = encrypt_object(&key, &context, &mutated).expect("mutated prune mark encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_prune_mark_plaintext(
    opened: &fileferry_core::OpenedRepository,
    object_name: &str,
    plaintext: &[u8],
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(KeyPurpose::PruneMark, FORGET_PRUNE_REPOSITORY_ID.as_bytes())
        .expect("prune subkey");
    let context = ObjectContext::new(ObjectKind::PruneMark, object_name).expect("prune context");
    let encrypted = encrypt_object(&key, &context, plaintext).expect("prune mark encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_policy_config_value(
    opened: &fileferry_core::OpenedRepository,
    object_name: &str,
    bytes: &[u8],
    mutate: impl FnOnce(&mut Value),
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(
            KeyPurpose::PolicyConfig,
            POLICY_CONFIG_REPOSITORY_ID.as_bytes(),
        )
        .expect("policy config subkey");
    let context =
        ObjectContext::new(ObjectKind::PolicyConfig, object_name).expect("policy config context");
    let plaintext = decrypt_object(&key, &context, &decode_fixture_encrypted_object(bytes))
        .expect("fixture policy config decrypts");
    let mut value: Value = serde_json::from_slice(&plaintext).expect("fixture policy config json");
    mutate(&mut value);
    let mutated = serde_json::to_vec(&value).expect("mutated policy config json");
    let encrypted =
        encrypt_object(&key, &context, &mutated).expect("mutated policy config encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_policy_config_plaintext(
    opened: &fileferry_core::OpenedRepository,
    object_name: &str,
    plaintext: &[u8],
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(
            KeyPurpose::PolicyConfig,
            POLICY_CONFIG_REPOSITORY_ID.as_bytes(),
        )
        .expect("policy config subkey");
    let context =
        ObjectContext::new(ObjectKind::PolicyConfig, object_name).expect("policy config context");
    let encrypted = encrypt_object(&key, &context, plaintext).expect("policy config encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_upload_state_value(
    opened: &fileferry_core::OpenedRepository,
    object_name: &str,
    bytes: &[u8],
    mutate: impl FnOnce(&mut Value),
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(
            KeyPurpose::UploadState,
            UPLOAD_STATE_REPOSITORY_ID.as_bytes(),
        )
        .expect("upload state subkey");
    let context =
        ObjectContext::new(ObjectKind::UploadState, object_name).expect("upload state context");
    let plaintext = decrypt_object(&key, &context, &decode_fixture_encrypted_object(bytes))
        .expect("fixture upload state decrypts");
    let mut value: Value = serde_json::from_slice(&plaintext).expect("fixture upload state json");
    mutate(&mut value);
    let mutated = serde_json::to_vec(&value).expect("mutated upload state json");
    let encrypted =
        encrypt_object(&key, &context, &mutated).expect("mutated upload state encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_upload_state_plaintext(
    opened: &fileferry_core::OpenedRepository,
    object_name: &str,
    plaintext: &[u8],
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(
            KeyPurpose::UploadState,
            UPLOAD_STATE_REPOSITORY_ID.as_bytes(),
        )
        .expect("upload state subkey");
    let context =
        ObjectContext::new(ObjectKind::UploadState, object_name).expect("upload state context");
    let encrypted = encrypt_object(&key, &context, plaintext).expect("upload state encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_lease_state_value(
    opened: &fileferry_core::OpenedRepository,
    object_name: &str,
    bytes: &[u8],
    mutate: impl FnOnce(&mut Value),
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(KeyPurpose::LeaseState, LEASE_STATE_REPOSITORY_ID.as_bytes())
        .expect("lease state subkey");
    let context =
        ObjectContext::new(ObjectKind::LeaseState, object_name).expect("lease state context");
    let plaintext = decrypt_object(&key, &context, &decode_fixture_encrypted_object(bytes))
        .expect("fixture lease state decrypts");
    let mut value: Value = serde_json::from_slice(&plaintext).expect("fixture lease state json");
    mutate(&mut value);
    let mutated = serde_json::to_vec(&value).expect("mutated lease state json");
    let encrypted = encrypt_object(&key, &context, &mutated).expect("mutated lease state encrypts");
    encode_fixture_encrypted_object(encrypted)
}

fn reencrypt_lease_state_plaintext(
    opened: &fileferry_core::OpenedRepository,
    object_name: &str,
    plaintext: &[u8],
) -> Vec<u8> {
    let key = opened
        .master_key
        .derive_subkey(KeyPurpose::LeaseState, LEASE_STATE_REPOSITORY_ID.as_bytes())
        .expect("lease state subkey");
    let context =
        ObjectContext::new(ObjectKind::LeaseState, object_name).expect("lease state context");
    let encrypted = encrypt_object(&key, &context, plaintext).expect("lease state encrypts");
    encode_fixture_encrypted_object(encrypted)
}

#[tokio::test]
async fn bootstrap_key_slot_fixture_opens_with_primary_passphrase() {
    let store = load_bootstrap_and_key_slot().await;

    let opened = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect("fixture opens with primary passphrase");

    assert_eq!(opened.repository_id, REPOSITORY_ID);
    assert_eq!(opened.key_slots, 2);
    assert_eq!(opened.unlocked_key_slot_id, None);
}

#[tokio::test]
async fn external_key_slot_fixture_opens_with_added_passphrase() {
    let store = load_bootstrap_and_key_slot().await;

    let opened = open_repository(&store, &secret(ADDED_PASSPHRASE))
        .await
        .expect("fixture opens with added passphrase");

    assert_eq!(opened.repository_id, REPOSITORY_ID);
    assert_eq!(opened.key_slots, 2);
    assert_eq!(opened.unlocked_key_slot_id.as_deref(), Some(KEY_SLOT_ID));
}

#[tokio::test]
async fn key_slot_removal_fixture_hides_removed_slot() {
    let store = load_complete_fixture().await;

    let opened = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect("fixture opens with remaining primary passphrase");
    assert_eq!(opened.repository_id, REPOSITORY_ID);
    assert_eq!(opened.key_slots, 1);

    let removed = open_repository(&store, &secret(ADDED_PASSPHRASE))
        .await
        .expect_err("removed key slot passphrase fails closed");
    assert!(matches!(removed, CoreError::RepositoryUnlock { .. }));
}

#[tokio::test]
async fn malformed_bootstrap_fixture_variant_is_rejected() {
    let store = load_bootstrap_and_key_slot().await;
    store
        .overwrite_for_tests(object_key("bootstrap"), b"not-json".to_vec())
        .await;

    let error = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect_err("malformed bootstrap is rejected");

    assert!(matches!(error, CoreError::RepositoryBootstrapDecode { .. }));
}

#[tokio::test]
async fn unsupported_bootstrap_fixture_version_is_rejected() {
    let store = load_bootstrap_and_key_slot().await;
    let mut bootstrap: Value = serde_json::from_slice(BOOTSTRAP).expect("fixture bootstrap json");
    bootstrap["format_version"] = json!(999);
    store
        .overwrite_for_tests(
            object_key("bootstrap"),
            serde_json::to_vec_pretty(&bootstrap).expect("unsupported version bootstrap json"),
        )
        .await;

    let error = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect_err("unsupported bootstrap version is rejected");

    assert!(matches!(
        error,
        CoreError::UnsupportedRepositoryFormat {
            format_version: 999
        }
    ));
}

#[tokio::test]
async fn migration_current_v0_fixture_is_detected_as_current() {
    let store = load_bootstrap_and_key_slot().await;

    let inspection = inspect_repository_format(&store)
        .await
        .expect("format inspection succeeds for v0 fixture");

    assert_eq!(inspection.format_version, 0);
    assert_eq!(inspection.latest_supported_format_version, 0);
    assert_eq!(
        inspection.compatibility,
        RepositoryFormatCompatibility::Current
    );
    assert!(inspection.features.is_empty());

    let opened = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect("current v0 fixture still opens");
    assert_eq!(opened.repository_id, REPOSITORY_ID);
}

#[tokio::test]
async fn migration_future_format_fixture_is_detected_and_rejected() {
    let store = load_migration_bootstrap_fixture(MIGRATION_FUTURE_FORMAT_BOOTSTRAP).await;

    let inspection = inspect_repository_format(&store)
        .await
        .expect("future format bootstrap can be inspected without unlock");
    assert_eq!(inspection.format_version, 1);
    assert_eq!(inspection.latest_supported_format_version, 0);
    assert_eq!(
        inspection.compatibility,
        RepositoryFormatCompatibility::UnsupportedFuture
    );

    let error = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect_err("future repository format is rejected before unlock");
    assert!(matches!(
        error,
        CoreError::UnsupportedRepositoryFormat { format_version: 1 }
    ));
}

#[tokio::test]
async fn migration_future_feature_fixture_is_detected_and_rejected() {
    let store = load_migration_bootstrap_fixture(MIGRATION_FUTURE_FEATURE_BOOTSTRAP).await;

    let inspection = inspect_repository_format(&store)
        .await
        .expect("future feature bootstrap can be inspected without unlock");
    assert_eq!(inspection.format_version, 0);
    assert_eq!(
        inspection.compatibility,
        RepositoryFormatCompatibility::UnsupportedFeatures
    );
    assert_eq!(
        inspection.features,
        vec!["requires-migration-v1".to_owned()]
    );

    let error = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect_err("supported format with unknown feature flags is rejected before unlock");
    assert!(matches!(error, CoreError::UnsupportedRepositoryFeatures));
}

#[tokio::test]
async fn migration_unversioned_bootstrap_fixture_is_rejected_as_pre_v0() {
    let store = load_migration_bootstrap_fixture(MIGRATION_UNVERSIONED_BOOTSTRAP).await;

    let inspection_error = inspect_repository_format(&store)
        .await
        .expect_err("unversioned bootstrap has no migration path");
    assert!(matches!(
        inspection_error,
        CoreError::InvalidRepositoryBootstrap {
            reason: "repository format version is missing"
        }
    ));

    let open_error = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect_err("unversioned bootstrap is rejected through normal open");
    assert!(matches!(
        open_error,
        CoreError::InvalidRepositoryBootstrap {
            reason: "repository format version is missing"
        }
    ));
}

#[tokio::test]
async fn migration_format_inspection_rejects_malformed_bootstrap_json() {
    let store = load_migration_bootstrap_fixture(b"not-json").await;

    let error = inspect_repository_format(&store)
        .await
        .expect_err("malformed bootstrap is rejected by format inspection");

    assert!(matches!(error, CoreError::RepositoryBootstrapDecode { .. }));
}

#[tokio::test]
async fn tampered_external_key_slot_fixture_variant_is_rejected() {
    let store = load_bootstrap_and_key_slot().await;
    let mut key_slot: Value = serde_json::from_slice(KEY_SLOT).expect("fixture key slot json");
    key_slot["master_key_check"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");
    store
        .overwrite_for_tests(
            key_slot_object_key(),
            serde_json::to_vec_pretty(&key_slot).expect("tampered key slot json"),
        )
        .await;

    let error = open_repository(&store, &secret(ADDED_PASSPHRASE))
        .await
        .expect_err("tampered key slot is rejected");

    assert!(matches!(
        error,
        CoreError::InvalidKeySlot {
            reason: "key slot does not unlock this repository master key",
            ..
        }
    ));
}

#[tokio::test]
async fn tampered_key_slot_removal_fixture_variant_is_rejected() {
    let store = load_complete_fixture().await;
    let mut marker: Value =
        serde_json::from_slice(KEY_SLOT_REMOVAL).expect("fixture removal marker json");
    marker["master_key_removal_check"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");
    store
        .overwrite_for_tests(
            key_slot_removal_object_key(),
            serde_json::to_vec_pretty(&marker).expect("tampered removal marker json"),
        )
        .await;

    let error = open_repository(&store, &secret(PRIMARY_PASSPHRASE))
        .await
        .expect_err("tampered removal marker is rejected");

    assert!(matches!(
        error,
        CoreError::InvalidKeySlotRemoval {
            reason: "key-slot removal marker failed authentication",
            ..
        }
    ));
}

#[test]
fn recovery_export_fixture_verifies_with_primary_passphrase() {
    let verified = verify_repository_recovery_export(RECOVERY_EXPORT, &secret(PRIMARY_PASSPHRASE))
        .expect("recovery export verifies");

    assert_eq!(verified.repository_id, REPOSITORY_ID);
    assert_eq!(
        verified.export_id,
        "446e8fae656c955bbee4bf035ab73cb094ee74a08e5b13debe69a11a09d6a8bc"
    );
    assert_eq!(verified.created_at_unix_seconds, 1_779_376_246);
    assert_eq!(verified.kdf.memory_cost_kib, 65_536);
    assert_eq!(verified.kdf.time_cost, 3);
    assert_eq!(verified.kdf.parallelism, 4);
    assert_eq!(verified.aead, RepositoryAeadAlgorithm::XChaCha20Poly1305);
}

#[test]
fn malformed_recovery_export_fixture_variant_is_rejected() {
    let error = verify_repository_recovery_export(b"not-json", &secret(PRIMARY_PASSPHRASE))
        .expect_err("malformed recovery export is rejected");

    assert!(matches!(error, CoreError::RecoveryExportDecode { .. }));
}

#[test]
fn unsupported_recovery_export_fixture_version_is_rejected() {
    let mut export: Value =
        serde_json::from_slice(RECOVERY_EXPORT).expect("fixture recovery export json");
    export["format_version"] = json!(999);

    let error = verify_repository_recovery_export(
        &serde_json::to_vec_pretty(&export).expect("unsupported version recovery export json"),
        &secret(PRIMARY_PASSPHRASE),
    )
    .expect_err("unsupported recovery export version is rejected");

    assert!(matches!(
        error,
        CoreError::UnsupportedRepositoryFormat {
            format_version: 999
        }
    ));
}

#[test]
fn tampered_recovery_export_ciphertext_variant_is_rejected() {
    let mut export: Value =
        serde_json::from_slice(RECOVERY_EXPORT).expect("fixture recovery export json");
    export["recovery_key"]["wrapped_master_key"][0] = json!(0);

    let error = verify_repository_recovery_export(
        &serde_json::to_vec_pretty(&export).expect("tampered recovery export json"),
        &secret(PRIMARY_PASSPHRASE),
    )
    .expect_err("tampered recovery export ciphertext is rejected");

    assert!(matches!(error, CoreError::RepositoryUnlock { .. }));
}

#[test]
fn tampered_recovery_export_master_key_check_variant_is_rejected() {
    let mut export: Value =
        serde_json::from_slice(RECOVERY_EXPORT).expect("fixture recovery export json");
    export["master_key_check"] =
        json!("0000000000000000000000000000000000000000000000000000000000000000");

    let error = verify_repository_recovery_export(
        &serde_json::to_vec_pretty(&export).expect("tampered recovery export check json"),
        &secret(PRIMARY_PASSPHRASE),
    )
    .expect_err("tampered recovery export master-key check is rejected");

    assert!(matches!(
        error,
        CoreError::InvalidRecoveryExport {
            reason: "recovery export does not unlock this repository master key"
        }
    ));
}

#[tokio::test]
async fn snapshot_data_fixture_reads_checks_and_restores() {
    let store = load_snapshot_data_fixture().await;
    let opened = open_repository(&store, &secret(SNAPSHOT_DATA_PASSPHRASE))
        .await
        .expect("snapshot-data fixture opens");
    assert_eq!(opened.repository_id, SNAPSHOT_DATA_REPOSITORY_ID);

    let pipeline = snapshot_data_pipeline();
    let manifests = pipeline
        .read_committed_snapshot_manifests(&store, &opened.master_key)
        .await
        .expect("committed manifest reads");
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].snapshot_id, SNAPSHOT_ID);
    assert_eq!(manifests[0].body.tags, vec!["fixture-v0"]);
    assert_eq!(manifests[0].body.entries.len(), 4);

    let index = pipeline
        .read_chunk_index(&store, &opened.master_key, INDEX_ID)
        .await
        .expect("chunk index reads");
    assert_eq!(index.index_id, INDEX_ID);
    assert_eq!(index.chunks.len(), 2);

    let check = pipeline
        .check_repository_with_options(&store, &opened.master_key, CheckRepositoryOptions::full())
        .await
        .expect("repository check verifies fixture");
    assert_eq!(check.repository_id, SNAPSHOT_DATA_REPOSITORY_ID);
    assert_eq!(check.metadata_objects_checked, 3);
    assert_eq!(check.chunk_objects_checked, 2);

    let restored = pipeline
        .restore_snapshot_contents(
            &store,
            &opened.master_key,
            RestoreContentRequest {
                snapshot_id: SNAPSHOT_ID.to_owned(),
                paths: vec![PathBuf::from("data.txt")],
            },
        )
        .await
        .expect("fixture restores selected file");
    assert_eq!(restored.files.len(), 1);
    assert_eq!(
        restored.files[0].contents,
        b"chunk/index/manifest fixture data\nsecond line\n"
    );
}

#[tokio::test]
async fn snapshot_data_fixture_rejects_malformed_commit_marker() {
    let store = load_snapshot_data_fixture().await;
    store
        .overwrite_for_tests(object_key(COMMIT_OBJECT), b"not-json".to_vec())
        .await;
    let opened = open_repository(&store, &secret(SNAPSHOT_DATA_PASSPHRASE))
        .await
        .expect("snapshot-data fixture opens");

    let error = snapshot_data_pipeline()
        .read_committed_snapshot_manifests(&store, &opened.master_key)
        .await
        .expect_err("malformed commit marker is rejected");

    assert!(matches!(error, CoreError::CommitDecode { .. }));
}

#[tokio::test]
async fn snapshot_data_fixture_rejects_malformed_encrypted_framing_and_metadata() {
    let store = load_snapshot_data_fixture().await;
    let opened = open_repository(&store, &secret(SNAPSHOT_DATA_PASSPHRASE))
        .await
        .expect("snapshot-data fixture opens");
    let pipeline = snapshot_data_pipeline();

    store
        .overwrite_for_tests(object_key(MANIFEST_OBJECT), b"not-json".to_vec())
        .await;
    let framing_error = pipeline
        .read_snapshot_manifest(&store, &opened.master_key, SNAPSHOT_ID)
        .await
        .expect_err("malformed encrypted object framing is rejected");
    assert!(matches!(framing_error, CoreError::ObjectDecode { .. }));

    store
        .overwrite_for_tests(
            object_key(MANIFEST_OBJECT),
            reencrypt_snapshot_manifest_plaintext(&opened, br#"{"schema_version":"bad"}"#),
        )
        .await;
    let metadata_error = pipeline
        .read_snapshot_manifest(&store, &opened.master_key, SNAPSHOT_ID)
        .await
        .expect_err("malformed decrypted manifest metadata is rejected");
    assert!(matches!(metadata_error, CoreError::MetadataDecode { .. }));
}

#[tokio::test]
async fn snapshot_data_fixture_rejects_tampered_encrypted_manifest_index_and_chunk() {
    let opened_store = load_snapshot_data_fixture().await;
    let opened = open_repository(&opened_store, &secret(SNAPSHOT_DATA_PASSPHRASE))
        .await
        .expect("snapshot-data fixture opens");

    let manifest_store = load_snapshot_data_fixture().await;
    manifest_store
        .overwrite_for_tests(
            object_key(MANIFEST_OBJECT),
            tamper_fixture_ciphertext(SNAPSHOT_DATA_MANIFEST),
        )
        .await;
    let manifest_error = snapshot_data_pipeline()
        .read_snapshot_manifest(&manifest_store, &opened.master_key, SNAPSHOT_ID)
        .await
        .expect_err("tampered manifest authentication fails");
    assert!(matches!(
        manifest_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == MANIFEST_OBJECT
    ));

    let index_store = load_snapshot_data_fixture().await;
    index_store
        .overwrite_for_tests(
            object_key(INDEX_OBJECT),
            tamper_fixture_ciphertext(SNAPSHOT_DATA_INDEX),
        )
        .await;
    let index_error = snapshot_data_pipeline()
        .read_chunk_index(&index_store, &opened.master_key, INDEX_ID)
        .await
        .expect_err("tampered index authentication fails");
    assert!(matches!(
        index_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == INDEX_OBJECT
    ));

    let chunk_store = load_snapshot_data_fixture().await;
    chunk_store
        .overwrite_for_tests(
            object_key(FIRST_CHUNK_OBJECT),
            tamper_fixture_ciphertext(SNAPSHOT_DATA_FIRST_CHUNK),
        )
        .await;
    let chunk_error = snapshot_data_pipeline()
        .check_repository(&chunk_store, &opened.master_key)
        .await
        .expect_err("tampered chunk authentication fails during check");
    assert!(matches!(
        chunk_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == FIRST_CHUNK_OBJECT
    ));
}

#[tokio::test]
async fn snapshot_data_fixture_rejects_wrong_authenticated_name_or_kind() {
    let store = load_snapshot_data_fixture().await;
    let opened = open_repository(&store, &secret(SNAPSHOT_DATA_PASSPHRASE))
        .await
        .expect("snapshot-data fixture opens");
    let pipeline = snapshot_data_pipeline();

    let wrong_snapshot_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let wrong_manifest_object =
        "objects/manifest/ff/ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    store
        .overwrite_for_tests(
            object_key(wrong_manifest_object),
            SNAPSHOT_DATA_MANIFEST.to_vec(),
        )
        .await;
    let wrong_name_error = pipeline
        .read_snapshot_manifest(&store, &opened.master_key, wrong_snapshot_id)
        .await
        .expect_err("manifest bytes under the wrong object name fail authentication");
    assert!(matches!(
        wrong_name_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == wrong_manifest_object
    ));

    store
        .overwrite_for_tests(object_key(MANIFEST_OBJECT), SNAPSHOT_DATA_INDEX.to_vec())
        .await;
    let wrong_kind_error = pipeline
        .read_snapshot_manifest(&store, &opened.master_key, SNAPSHOT_ID)
        .await
        .expect_err("index bytes read as a manifest fail authentication");
    assert!(matches!(
        wrong_kind_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == MANIFEST_OBJECT
    ));
}

#[tokio::test]
async fn snapshot_data_fixture_rejects_metadata_identity_mismatches() {
    let manifest_store = load_snapshot_data_fixture().await;
    let opened = open_repository(&manifest_store, &secret(SNAPSHOT_DATA_PASSPHRASE))
        .await
        .expect("snapshot-data fixture opens");
    manifest_store
        .overwrite_for_tests(
            object_key(MANIFEST_OBJECT),
            reencrypt_snapshot_manifest_variant(&opened, |manifest| {
                manifest.body.tags.push("mismatched-identity".to_owned());
            }),
        )
        .await;
    let manifest_error = snapshot_data_pipeline()
        .read_snapshot_manifest(&manifest_store, &opened.master_key, SNAPSHOT_ID)
        .await
        .expect_err("manifest body identity mismatch is rejected");
    assert!(matches!(
        manifest_error,
        CoreError::MetadataIdentityMismatch {
            kind: "snapshot manifest",
            object_key,
            ..
        } if object_key.as_str() == MANIFEST_OBJECT
    ));

    let index_store = load_snapshot_data_fixture().await;
    index_store
        .overwrite_for_tests(
            object_key(INDEX_OBJECT),
            reencrypt_chunk_index_variant(&opened, |index| {
                index.chunks.clear();
            }),
        )
        .await;
    let index_error = snapshot_data_pipeline()
        .read_chunk_index(&index_store, &opened.master_key, INDEX_ID)
        .await
        .expect_err("index contents identity mismatch is rejected");
    assert!(matches!(
        index_error,
        CoreError::MetadataIdentityMismatch {
            kind: "chunk index",
            object_key,
            ..
        } if object_key.as_str() == INDEX_OBJECT
    ));
}

#[tokio::test]
async fn snapshot_data_fixture_rejects_unsupported_schema_versions() {
    let store = load_snapshot_data_fixture().await;
    let mut commit: Value = serde_json::from_slice(SNAPSHOT_DATA_COMMIT).expect("fixture commit");
    commit["schema_version"] = json!(999);
    store
        .overwrite_for_tests(
            object_key(COMMIT_OBJECT),
            serde_json::to_vec(&commit).expect("unsupported commit schema json"),
        )
        .await;
    let opened = open_repository(&store, &secret(SNAPSHOT_DATA_PASSPHRASE))
        .await
        .expect("snapshot-data fixture opens");
    let commit_error = snapshot_data_pipeline()
        .read_committed_snapshot_manifests(&store, &opened.master_key)
        .await
        .expect_err("unsupported commit marker schema is rejected");
    assert!(matches!(
        commit_error,
        CoreError::InvalidCommitMarker {
            reason: "unsupported commit marker schema version",
            ..
        }
    ));

    let manifest_store = load_snapshot_data_fixture().await;
    manifest_store
        .overwrite_for_tests(
            object_key(MANIFEST_OBJECT),
            reencrypt_snapshot_manifest_variant(&opened, |manifest| {
                manifest.schema_version = 999;
            }),
        )
        .await;
    let manifest_error = snapshot_data_pipeline()
        .read_snapshot_manifest(&manifest_store, &opened.master_key, SNAPSHOT_ID)
        .await
        .expect_err("unsupported manifest schema is rejected");
    assert!(matches!(
        manifest_error,
        CoreError::InvalidSnapshotManifest {
            reason: "unsupported snapshot manifest schema version",
            ..
        }
    ));

    let index_store = load_snapshot_data_fixture().await;
    index_store
        .overwrite_for_tests(
            object_key(INDEX_OBJECT),
            reencrypt_chunk_index_variant(&opened, |index| {
                index.schema_version = 999;
            }),
        )
        .await;
    let index_error = snapshot_data_pipeline()
        .read_chunk_index(&index_store, &opened.master_key, INDEX_ID)
        .await
        .expect_err("unsupported index schema is rejected");
    assert!(matches!(
        index_error,
        CoreError::InvalidChunkIndex {
            reason: "unsupported chunk index schema version",
            ..
        }
    ));
}

#[tokio::test]
async fn forget_prune_state_fixture_reads_validates_and_checks_retained_snapshot() {
    let store = load_forget_prune_state_fixture().await;
    let opened = open_repository(&store, &secret(FORGET_PRUNE_PASSPHRASE))
        .await
        .expect("forget-prune fixture opens");
    assert_eq!(opened.repository_id, FORGET_PRUNE_REPOSITORY_ID);

    let pipeline = forget_prune_pipeline();
    let forgotten = pipeline
        .read_forgotten_snapshot_ids(&store)
        .await
        .expect("forget marker reads");
    assert!(forgotten.contains(FORGOTTEN_SNAPSHOT_ID));

    let plan = pipeline
        .read_prune_plan_state(&store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect("prune plan reads");
    assert_eq!(plan.repository_id, FORGET_PRUNE_REPOSITORY_ID);
    assert_eq!(plan.plan_id, PRUNE_PLAN_ID);
    assert_eq!(plan.candidate_objects.len(), 5);
    assert!(
        plan.candidate_objects
            .iter()
            .any(|object| object.object_key == FORGET_MARKER_OBJECT
                && object.kind == PruneObjectKind::ForgetMarker)
    );

    let completion = pipeline
        .read_prune_completion_state(&store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect("prune completion reads");
    assert_eq!(completion.repository_id, FORGET_PRUNE_REPOSITORY_ID);
    assert_eq!(completion.plan_id, PRUNE_PLAN_ID);
    assert_eq!(completion.candidate_objects, plan.candidate_objects.len());
    assert_eq!(completion.deleted_objects, plan.candidate_objects.len());
    assert_eq!(completion.missing_objects, 0);

    let manifests = pipeline
        .read_committed_snapshot_manifests(&store, &opened.master_key)
        .await
        .expect("retained committed snapshot reads");
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0].snapshot_id, RETAINED_SNAPSHOT_ID);

    let check = pipeline
        .check_repository_with_options(&store, &opened.master_key, CheckRepositoryOptions::full())
        .await
        .expect("retained repository data checks");
    assert_eq!(check.repository_id, FORGET_PRUNE_REPOSITORY_ID);
    assert_eq!(check.errors, Vec::new());
}

#[tokio::test]
async fn forget_prune_state_fixture_rejects_malformed_forget_marker() {
    let store = load_forget_prune_state_fixture().await;
    store
        .overwrite_for_tests(object_key(FORGET_MARKER_OBJECT), b"not-json".to_vec())
        .await;

    let error = forget_prune_pipeline()
        .read_forgotten_snapshot_ids(&store)
        .await
        .expect_err("malformed forget marker is rejected");

    assert!(matches!(error, CoreError::ForgetMarkerDecode { .. }));
}

#[tokio::test]
async fn forget_prune_state_fixture_rejects_forget_marker_identity_and_schema_mismatches() {
    let unsupported_store = load_forget_prune_state_fixture().await;
    let mut marker: Value =
        serde_json::from_slice(FORGET_PRUNE_FORGET_MARKER).expect("fixture forget marker json");
    marker["schema_version"] = json!(999);
    unsupported_store
        .overwrite_for_tests(
            object_key(FORGET_MARKER_OBJECT),
            serde_json::to_vec(&marker).expect("unsupported forget marker json"),
        )
        .await;
    let unsupported = forget_prune_pipeline()
        .read_forgotten_snapshot_ids(&unsupported_store)
        .await
        .expect_err("unsupported forget marker schema is rejected");
    assert!(matches!(
        unsupported,
        CoreError::InvalidForgetMarker {
            reason: "unsupported forget marker schema version",
            ..
        }
    ));

    let mismatch_store = load_forget_prune_state_fixture().await;
    let mut marker: Value =
        serde_json::from_slice(FORGET_PRUNE_FORGET_MARKER).expect("fixture forget marker json");
    marker["manifest_object"] = json!(RETAINED_MANIFEST_OBJECT);
    mismatch_store
        .overwrite_for_tests(
            object_key(FORGET_MARKER_OBJECT),
            serde_json::to_vec(&marker).expect("mismatched forget marker json"),
        )
        .await;
    let mismatch = forget_prune_pipeline()
        .read_forgotten_snapshot_ids(&mismatch_store)
        .await
        .expect_err("forget marker metadata identity mismatch is rejected");
    assert!(matches!(
        mismatch,
        CoreError::InvalidForgetMarker {
            reason: "forget marker manifest object does not match snapshot id",
            ..
        }
    ));
}

#[tokio::test]
async fn forget_prune_state_fixture_rejects_malformed_prune_framing_and_metadata() {
    let store = load_forget_prune_state_fixture().await;
    let opened = open_repository(&store, &secret(FORGET_PRUNE_PASSPHRASE))
        .await
        .expect("forget-prune fixture opens");
    let pipeline = forget_prune_pipeline();

    store
        .overwrite_for_tests(object_key(PRUNE_PLAN_OBJECT), b"not-json".to_vec())
        .await;
    let framing_error = pipeline
        .read_prune_plan_state(&store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect_err("malformed prune plan frame is rejected");
    assert!(matches!(framing_error, CoreError::PrunePlanDecode { .. }));

    store
        .overwrite_for_tests(
            object_key(PRUNE_PLAN_OBJECT),
            reencrypt_prune_mark_plaintext(
                &opened,
                PRUNE_PLAN_OBJECT,
                br#"{"schema_version":"bad"}"#,
            ),
        )
        .await;
    let metadata_error = pipeline
        .read_prune_plan_state(&store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect_err("malformed decrypted prune plan metadata is rejected");
    assert!(matches!(metadata_error, CoreError::PrunePlanDecode { .. }));
}

#[tokio::test]
async fn forget_prune_state_fixture_rejects_tampered_prune_plan_and_completion() {
    let opened_store = load_forget_prune_state_fixture().await;
    let opened = open_repository(&opened_store, &secret(FORGET_PRUNE_PASSPHRASE))
        .await
        .expect("forget-prune fixture opens");
    let pipeline = forget_prune_pipeline();

    let plan_store = load_forget_prune_state_fixture().await;
    plan_store
        .overwrite_for_tests(
            object_key(PRUNE_PLAN_OBJECT),
            tamper_fixture_ciphertext(FORGET_PRUNE_PLAN),
        )
        .await;
    let plan_error = pipeline
        .read_prune_plan_state(&plan_store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect_err("tampered prune plan authentication fails");
    assert!(matches!(
        plan_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == PRUNE_PLAN_OBJECT
    ));

    let completion_store = load_forget_prune_state_fixture().await;
    completion_store
        .overwrite_for_tests(
            object_key(PRUNE_COMPLETION_OBJECT),
            tamper_fixture_ciphertext(FORGET_PRUNE_COMPLETION),
        )
        .await;
    let completion_error = pipeline
        .read_prune_completion_state(&completion_store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect_err("tampered prune completion authentication fails");
    assert!(matches!(
        completion_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == PRUNE_COMPLETION_OBJECT
    ));

    let resume_error = pipeline
        .prune_repository(
            &completion_store,
            &opened.master_key,
            PruneRepositoryOptions::sweep(),
        )
        .await
        .expect_err("tampered completion is rejected during prune recovery scan");
    assert!(matches!(
        resume_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == PRUNE_COMPLETION_OBJECT
    ));
}

#[tokio::test]
async fn forget_prune_state_fixture_rejects_wrong_authenticated_name_or_kind() {
    let store = load_forget_prune_state_fixture().await;
    let opened = open_repository(&store, &secret(FORGET_PRUNE_PASSPHRASE))
        .await
        .expect("forget-prune fixture opens");
    let pipeline = forget_prune_pipeline();

    let wrong_plan_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let wrong_plan_object =
        "objects/prune-plan/ff/ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    store
        .overwrite_for_tests(object_key(wrong_plan_object), FORGET_PRUNE_PLAN.to_vec())
        .await;
    let wrong_name_error = pipeline
        .read_prune_plan_state(&store, &opened.master_key, wrong_plan_id)
        .await
        .expect_err("prune plan bytes under the wrong object name fail authentication");
    assert!(matches!(
        wrong_name_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == wrong_plan_object
    ));

    let prune_key = opened
        .master_key
        .derive_subkey(KeyPurpose::PruneMark, FORGET_PRUNE_REPOSITORY_ID.as_bytes())
        .expect("prune subkey");
    let wrong_kind_context =
        ObjectContext::new(ObjectKind::Index, PRUNE_PLAN_OBJECT).expect("wrong-kind context");
    let wrong_kind = decrypt_object(
        &prune_key,
        &wrong_kind_context,
        &decode_fixture_encrypted_object(FORGET_PRUNE_PLAN),
    );
    assert!(wrong_kind.is_err());
}

#[tokio::test]
async fn forget_prune_state_fixture_rejects_prune_metadata_identity_mismatches() {
    let plan_store = load_forget_prune_state_fixture().await;
    let opened = open_repository(&plan_store, &secret(FORGET_PRUNE_PASSPHRASE))
        .await
        .expect("forget-prune fixture opens");
    let pipeline = forget_prune_pipeline();

    plan_store
        .overwrite_for_tests(
            object_key(PRUNE_PLAN_OBJECT),
            reencrypt_prune_mark_value(&opened, PRUNE_PLAN_OBJECT, FORGET_PRUNE_PLAN, |plan| {
                plan["repository_id"] =
                    json!("0000000000000000000000000000000000000000000000000000000000000000");
            }),
        )
        .await;
    let plan_error = pipeline
        .read_prune_plan_state(&plan_store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect_err("prune plan repository mismatch is rejected");
    assert!(matches!(
        plan_error,
        CoreError::InvalidPrunePlan {
            reason: "prune plan repository id does not match this repository",
            ..
        }
    ));

    let completion_store = load_forget_prune_state_fixture().await;
    completion_store
        .overwrite_for_tests(
            object_key(PRUNE_COMPLETION_OBJECT),
            reencrypt_prune_mark_value(
                &opened,
                PRUNE_COMPLETION_OBJECT,
                FORGET_PRUNE_COMPLETION,
                |completion| {
                    completion["plan_object"] = json!(
                        "objects/prune-plan/ff/ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
                    );
                },
            ),
        )
        .await;
    let completion_error = pipeline
        .read_prune_completion_state(&completion_store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect_err("prune completion plan-object mismatch is rejected");
    assert!(matches!(
        completion_error,
        CoreError::InvalidPruneCompletion {
            reason: "prune completion plan object does not match plan id",
            ..
        }
    ));
}

#[tokio::test]
async fn forget_prune_state_fixture_rejects_unsupported_prune_versions() {
    let plan_store = load_forget_prune_state_fixture().await;
    let opened = open_repository(&plan_store, &secret(FORGET_PRUNE_PASSPHRASE))
        .await
        .expect("forget-prune fixture opens");
    let pipeline = forget_prune_pipeline();

    plan_store
        .overwrite_for_tests(
            object_key(PRUNE_PLAN_OBJECT),
            reencrypt_prune_mark_value(&opened, PRUNE_PLAN_OBJECT, FORGET_PRUNE_PLAN, |plan| {
                plan["schema_version"] = json!(999);
            }),
        )
        .await;
    let plan_schema_error = pipeline
        .read_prune_plan_state(&plan_store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect_err("unsupported prune plan schema is rejected");
    assert!(matches!(
        plan_schema_error,
        CoreError::InvalidPrunePlan {
            reason: "unsupported prune plan schema version",
            ..
        }
    ));

    let completion_store = load_forget_prune_state_fixture().await;
    completion_store
        .overwrite_for_tests(
            object_key(PRUNE_COMPLETION_OBJECT),
            reencrypt_prune_mark_value(
                &opened,
                PRUNE_COMPLETION_OBJECT,
                FORGET_PRUNE_COMPLETION,
                |completion| {
                    completion["format_version"] = json!(999);
                },
            ),
        )
        .await;
    let completion_format_error = pipeline
        .read_prune_completion_state(&completion_store, &opened.master_key, PRUNE_PLAN_ID)
        .await
        .expect_err("unsupported prune completion format is rejected");
    assert!(matches!(
        completion_format_error,
        CoreError::InvalidPruneCompletion {
            reason: "unsupported prune completion format version",
            ..
        }
    ));
}

#[tokio::test]
async fn forget_prune_state_fixture_rejects_stale_pending_prune_plan_replay() {
    let store = load_pending_prune_plan_fixture().await;
    let opened = open_repository(&store, &secret(FORGET_PRUNE_PASSPHRASE))
        .await
        .expect("forget-prune fixture opens");

    let error = forget_prune_pipeline()
        .prune_repository(&store, &opened.master_key, PruneRepositoryOptions::sweep())
        .await
        .expect_err("stale pending prune plan is rejected before deleting objects");

    assert!(matches!(
        error,
        CoreError::PruneRepositoryStateChanged { .. }
    ));
}

#[tokio::test]
async fn policy_config_fixture_reads_validates_and_rewrites_idempotently() {
    let store = load_policy_config_fixture().await;
    let opened = open_repository(&store, &secret(POLICY_CONFIG_PASSPHRASE))
        .await
        .expect("policy-config fixture opens");
    assert_eq!(opened.repository_id, POLICY_CONFIG_REPOSITORY_ID);

    let pipeline = policy_config_pipeline();
    let policy = pipeline
        .read_repository_policy_config(&store, &opened.master_key, POLICY_ID)
        .await
        .expect("policy config reads");
    assert_eq!(policy.repository_id, POLICY_CONFIG_REPOSITORY_ID);
    assert_eq!(policy.policy_id, POLICY_ID);
    assert_eq!(policy.body.created_at_unix_seconds, 1_779_380_000);
    assert_eq!(policy.body.retention.keep_last, Some(7));
    assert_eq!(policy.body.retention.keep_daily, Some(14));
    assert_eq!(policy.body.retention.keep_weekly, Some(8));
    assert_eq!(
        policy.body.retention.keep_tags,
        vec!["gold".to_owned(), "legal-hold".to_owned()]
    );

    let rewritten = pipeline
        .write_repository_policy_config(
            &store,
            &opened.master_key,
            RepositoryPolicyConfigRequest {
                created_at_unix_seconds: policy.body.created_at_unix_seconds,
                retention: policy.body.retention,
            },
        )
        .await
        .expect("same policy config write is idempotent");
    assert_eq!(rewritten.policy_id, POLICY_ID);
    assert_eq!(rewritten.policy_object.as_str(), POLICY_OBJECT);
    assert!(!rewritten.created);
    assert_eq!(rewritten.bytes_written, 0);
}

#[tokio::test]
async fn policy_config_fixture_rejects_malformed_framing_and_metadata() {
    let store = load_policy_config_fixture().await;
    let opened = open_repository(&store, &secret(POLICY_CONFIG_PASSPHRASE))
        .await
        .expect("policy-config fixture opens");
    let pipeline = policy_config_pipeline();

    store
        .overwrite_for_tests(object_key(POLICY_OBJECT), b"not-json".to_vec())
        .await;
    let framing_error = pipeline
        .read_repository_policy_config(&store, &opened.master_key, POLICY_ID)
        .await
        .expect_err("malformed policy config frame is rejected");
    assert!(matches!(
        framing_error,
        CoreError::PolicyConfigDecode { .. }
    ));

    store
        .overwrite_for_tests(
            object_key(POLICY_OBJECT),
            reencrypt_policy_config_plaintext(
                &opened,
                POLICY_OBJECT,
                br#"{"schema_version":"bad"}"#,
            ),
        )
        .await;
    let metadata_error = pipeline
        .read_repository_policy_config(&store, &opened.master_key, POLICY_ID)
        .await
        .expect_err("malformed decrypted policy config metadata is rejected");
    assert!(matches!(
        metadata_error,
        CoreError::PolicyConfigDecode { .. }
    ));
}

#[tokio::test]
async fn policy_config_fixture_rejects_tampered_ciphertext() {
    let opened_store = load_policy_config_fixture().await;
    let opened = open_repository(&opened_store, &secret(POLICY_CONFIG_PASSPHRASE))
        .await
        .expect("policy-config fixture opens");
    let store = load_policy_config_fixture().await;
    store
        .overwrite_for_tests(
            object_key(POLICY_OBJECT),
            tamper_fixture_ciphertext(POLICY_CONFIG),
        )
        .await;

    let error = policy_config_pipeline()
        .read_repository_policy_config(&store, &opened.master_key, POLICY_ID)
        .await
        .expect_err("tampered policy config authentication fails");

    assert!(matches!(
        error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == POLICY_OBJECT
    ));
}

#[tokio::test]
async fn policy_config_fixture_rejects_wrong_authenticated_name_or_kind() {
    let store = load_policy_config_fixture().await;
    let opened = open_repository(&store, &secret(POLICY_CONFIG_PASSPHRASE))
        .await
        .expect("policy-config fixture opens");
    let pipeline = policy_config_pipeline();

    let wrong_policy_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let wrong_policy_object =
        "objects/policy/ff/ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    store
        .overwrite_for_tests(object_key(wrong_policy_object), POLICY_CONFIG.to_vec())
        .await;
    let wrong_name_error = pipeline
        .read_repository_policy_config(&store, &opened.master_key, wrong_policy_id)
        .await
        .expect_err("policy bytes under the wrong object name fail authentication");
    assert!(matches!(
        wrong_name_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == wrong_policy_object
    ));

    let policy_key = opened
        .master_key
        .derive_subkey(
            KeyPurpose::PolicyConfig,
            POLICY_CONFIG_REPOSITORY_ID.as_bytes(),
        )
        .expect("policy config subkey");
    let wrong_kind_context =
        ObjectContext::new(ObjectKind::Index, POLICY_OBJECT).expect("wrong-kind context");
    let wrong_kind = decrypt_object(
        &policy_key,
        &wrong_kind_context,
        &decode_fixture_encrypted_object(POLICY_CONFIG),
    );
    assert!(wrong_kind.is_err());
}

#[tokio::test]
async fn policy_config_fixture_rejects_metadata_identity_mismatch() {
    let store = load_policy_config_fixture().await;
    let opened = open_repository(&store, &secret(POLICY_CONFIG_PASSPHRASE))
        .await
        .expect("policy-config fixture opens");
    store
        .overwrite_for_tests(
            object_key(POLICY_OBJECT),
            reencrypt_policy_config_value(&opened, POLICY_OBJECT, POLICY_CONFIG, |policy| {
                policy["body"]["retention"]["keep_last"] = json!(99);
            }),
        )
        .await;

    let error = policy_config_pipeline()
        .read_repository_policy_config(&store, &opened.master_key, POLICY_ID)
        .await
        .expect_err("policy config body identity mismatch is rejected");

    assert!(matches!(
        error,
        CoreError::MetadataIdentityMismatch {
            kind: "policy config",
            object_key,
            ..
        } if object_key.as_str() == POLICY_OBJECT
    ));
}

#[tokio::test]
async fn policy_config_fixture_rejects_unsupported_versions_and_repository_mismatch() {
    let opened_store = load_policy_config_fixture().await;
    let opened = open_repository(&opened_store, &secret(POLICY_CONFIG_PASSPHRASE))
        .await
        .expect("policy-config fixture opens");
    let pipeline = policy_config_pipeline();

    let schema_store = load_policy_config_fixture().await;
    schema_store
        .overwrite_for_tests(
            object_key(POLICY_OBJECT),
            reencrypt_policy_config_value(&opened, POLICY_OBJECT, POLICY_CONFIG, |policy| {
                policy["schema_version"] = json!(999);
            }),
        )
        .await;
    let schema_error = pipeline
        .read_repository_policy_config(&schema_store, &opened.master_key, POLICY_ID)
        .await
        .expect_err("unsupported policy config schema is rejected");
    assert!(matches!(
        schema_error,
        CoreError::InvalidPolicyConfig {
            reason: "unsupported policy config schema version",
            ..
        }
    ));

    let format_store = load_policy_config_fixture().await;
    format_store
        .overwrite_for_tests(
            object_key(POLICY_OBJECT),
            reencrypt_policy_config_value(&opened, POLICY_OBJECT, POLICY_CONFIG, |policy| {
                policy["format_version"] = json!(999);
            }),
        )
        .await;
    let format_error = pipeline
        .read_repository_policy_config(&format_store, &opened.master_key, POLICY_ID)
        .await
        .expect_err("unsupported policy config format is rejected");
    assert!(matches!(
        format_error,
        CoreError::InvalidPolicyConfig {
            reason: "unsupported policy config format version",
            ..
        }
    ));

    let repository_store = load_policy_config_fixture().await;
    repository_store
        .overwrite_for_tests(
            object_key(POLICY_OBJECT),
            reencrypt_policy_config_value(&opened, POLICY_OBJECT, POLICY_CONFIG, |policy| {
                policy["repository_id"] =
                    json!("0000000000000000000000000000000000000000000000000000000000000000");
            }),
        )
        .await;
    let repository_error = pipeline
        .read_repository_policy_config(&repository_store, &opened.master_key, POLICY_ID)
        .await
        .expect_err("policy config repository mismatch is rejected");
    assert!(matches!(
        repository_error,
        CoreError::InvalidPolicyConfig {
            reason: "policy config repository id does not match this repository",
            ..
        }
    ));
}

#[tokio::test]
async fn policy_config_fixture_rejects_invalid_retention_shape() {
    let store = load_policy_config_fixture().await;
    let opened = open_repository(&store, &secret(POLICY_CONFIG_PASSPHRASE))
        .await
        .expect("policy-config fixture opens");
    store
        .overwrite_for_tests(
            object_key(POLICY_OBJECT),
            reencrypt_policy_config_value(&opened, POLICY_OBJECT, POLICY_CONFIG, |policy| {
                policy["body"]["retention"] = json!({
                    "keep_last": null,
                    "keep_hourly": null,
                    "keep_daily": null,
                    "keep_weekly": null,
                    "keep_monthly": null,
                    "keep_yearly": null,
                    "keep_tags": []
                });
            }),
        )
        .await;

    let error = policy_config_pipeline()
        .read_repository_policy_config(&store, &opened.master_key, POLICY_ID)
        .await
        .expect_err("empty retention policy is rejected");

    assert!(matches!(
        error,
        CoreError::InvalidPolicyConfig {
            reason: "retention policy must include at least one keep rule",
            ..
        }
    ));
}

#[tokio::test]
async fn upload_state_fixture_reads_validates_and_rewrites_idempotently() {
    let store = load_upload_state_fixture().await;
    let opened = open_repository(&store, &secret(UPLOAD_STATE_PASSPHRASE))
        .await
        .expect("upload-state fixture opens");
    assert_eq!(opened.repository_id, UPLOAD_STATE_REPOSITORY_ID);

    let pipeline = upload_state_pipeline();
    let state = pipeline
        .read_repository_upload_state(&store, &opened.master_key, UPLOAD_WRITER_ID, UPLOAD_ID)
        .await
        .expect("upload state reads");
    assert_eq!(state.repository_id, UPLOAD_STATE_REPOSITORY_ID);
    assert_eq!(state.writer_id, UPLOAD_WRITER_ID);
    assert_eq!(state.upload_id, UPLOAD_ID);
    assert_eq!(state.created_at_unix_seconds, 1_779_381_000);
    assert_eq!(state.operation, RepositoryUploadOperation::BackupSnapshot);
    assert!(state.commit_objects.is_empty());
    assert!(state.forget_marker_objects.is_empty());
    assert_eq!(state.pending_objects.len(), 3);
    assert_eq!(
        state.pending_objects[0].kind,
        RepositoryUploadPendingObjectKind::Chunk
    );

    let resumed = pipeline
        .read_repository_upload_state_for_resume(
            &store,
            &opened.master_key,
            UPLOAD_WRITER_ID,
            UPLOAD_ID,
        )
        .await
        .expect("upload state is current for resume");
    assert_eq!(resumed, state);

    let rewritten = pipeline
        .write_repository_upload_state(
            &store,
            &opened.master_key,
            RepositoryUploadStateRequest {
                writer_id: state.writer_id,
                upload_id: state.upload_id,
                created_at_unix_seconds: state.created_at_unix_seconds,
                operation: state.operation,
                pending_objects: state.pending_objects,
            },
        )
        .await
        .expect("same upload state write is idempotent");
    assert_eq!(rewritten.upload_object.as_str(), UPLOAD_STATE_OBJECT);
    assert!(!rewritten.created);
    assert_eq!(rewritten.bytes_written, 0);
}

#[tokio::test]
async fn upload_state_fixture_rejects_malformed_framing_and_metadata() {
    let store = load_upload_state_fixture().await;
    let opened = open_repository(&store, &secret(UPLOAD_STATE_PASSPHRASE))
        .await
        .expect("upload-state fixture opens");
    let pipeline = upload_state_pipeline();

    store
        .overwrite_for_tests(object_key(UPLOAD_STATE_OBJECT), b"not-json".to_vec())
        .await;
    let framing_error = pipeline
        .read_repository_upload_state(&store, &opened.master_key, UPLOAD_WRITER_ID, UPLOAD_ID)
        .await
        .expect_err("malformed upload state frame is rejected");
    assert!(matches!(framing_error, CoreError::UploadStateDecode { .. }));

    store
        .overwrite_for_tests(
            object_key(UPLOAD_STATE_OBJECT),
            reencrypt_upload_state_plaintext(
                &opened,
                UPLOAD_STATE_OBJECT,
                br#"{"schema_version":"bad"}"#,
            ),
        )
        .await;
    let metadata_error = pipeline
        .read_repository_upload_state(&store, &opened.master_key, UPLOAD_WRITER_ID, UPLOAD_ID)
        .await
        .expect_err("malformed decrypted upload state metadata is rejected");
    assert!(matches!(
        metadata_error,
        CoreError::UploadStateDecode { .. }
    ));
}

#[tokio::test]
async fn upload_state_fixture_rejects_tampered_ciphertext() {
    let opened_store = load_upload_state_fixture().await;
    let opened = open_repository(&opened_store, &secret(UPLOAD_STATE_PASSPHRASE))
        .await
        .expect("upload-state fixture opens");
    let store = load_upload_state_fixture().await;
    store
        .overwrite_for_tests(
            object_key(UPLOAD_STATE_OBJECT),
            tamper_fixture_ciphertext(UPLOAD_STATE),
        )
        .await;

    let error = upload_state_pipeline()
        .read_repository_upload_state(&store, &opened.master_key, UPLOAD_WRITER_ID, UPLOAD_ID)
        .await
        .expect_err("tampered upload state authentication fails");

    assert!(matches!(
        error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == UPLOAD_STATE_OBJECT
    ));
}

#[tokio::test]
async fn upload_state_fixture_rejects_wrong_authenticated_name_or_kind() {
    let store = load_upload_state_fixture().await;
    let opened = open_repository(&store, &secret(UPLOAD_STATE_PASSPHRASE))
        .await
        .expect("upload-state fixture opens");
    let pipeline = upload_state_pipeline();

    let wrong_upload_id = "3333333333333333333333333333333333333333333333333333333333333333";
    let wrong_upload_object = "objects/upload/1111111111111111111111111111111111111111111111111111111111111111/3333333333333333333333333333333333333333333333333333333333333333";
    store
        .overwrite_for_tests(object_key(wrong_upload_object), UPLOAD_STATE.to_vec())
        .await;
    let wrong_name_error = pipeline
        .read_repository_upload_state(
            &store,
            &opened.master_key,
            UPLOAD_WRITER_ID,
            wrong_upload_id,
        )
        .await
        .expect_err("upload state bytes under the wrong object name fail authentication");
    assert!(matches!(
        wrong_name_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == wrong_upload_object
    ));

    let upload_key = opened
        .master_key
        .derive_subkey(
            KeyPurpose::UploadState,
            UPLOAD_STATE_REPOSITORY_ID.as_bytes(),
        )
        .expect("upload state subkey");
    let wrong_kind_context =
        ObjectContext::new(ObjectKind::Index, UPLOAD_STATE_OBJECT).expect("wrong-kind context");
    let wrong_kind = decrypt_object(
        &upload_key,
        &wrong_kind_context,
        &decode_fixture_encrypted_object(UPLOAD_STATE),
    );
    assert!(wrong_kind.is_err());
}

#[tokio::test]
async fn upload_state_fixture_rejects_identity_and_repository_mismatches() {
    let store = load_upload_state_fixture().await;
    let opened = open_repository(&store, &secret(UPLOAD_STATE_PASSPHRASE))
        .await
        .expect("upload-state fixture opens");

    store
        .overwrite_for_tests(
            object_key(UPLOAD_STATE_OBJECT),
            reencrypt_upload_state_value(&opened, UPLOAD_STATE_OBJECT, UPLOAD_STATE, |state| {
                state["pending_objects"][0]["bytes"] = json!(999);
            }),
        )
        .await;
    let identity_error = upload_state_pipeline()
        .read_repository_upload_state(&store, &opened.master_key, UPLOAD_WRITER_ID, UPLOAD_ID)
        .await
        .expect_err("upload state identity mismatch is rejected");
    assert!(matches!(
        identity_error,
        CoreError::MetadataIdentityMismatch {
            kind: "upload state",
            object_key,
            ..
        } if object_key.as_str() == UPLOAD_STATE_OBJECT
    ));

    let repository_store = load_upload_state_fixture().await;
    repository_store
        .overwrite_for_tests(
            object_key(UPLOAD_STATE_OBJECT),
            reencrypt_upload_state_value(&opened, UPLOAD_STATE_OBJECT, UPLOAD_STATE, |state| {
                state["repository_id"] =
                    json!("0000000000000000000000000000000000000000000000000000000000000000");
            }),
        )
        .await;
    let repository_error = upload_state_pipeline()
        .read_repository_upload_state(
            &repository_store,
            &opened.master_key,
            UPLOAD_WRITER_ID,
            UPLOAD_ID,
        )
        .await
        .expect_err("upload state repository mismatch is rejected");
    assert!(matches!(
        repository_error,
        CoreError::InvalidUploadState {
            reason: "upload state repository id does not match this repository",
            ..
        }
    ));
}

#[tokio::test]
async fn upload_state_fixture_rejects_unsupported_versions() {
    let opened_store = load_upload_state_fixture().await;
    let opened = open_repository(&opened_store, &secret(UPLOAD_STATE_PASSPHRASE))
        .await
        .expect("upload-state fixture opens");
    let pipeline = upload_state_pipeline();

    let schema_store = load_upload_state_fixture().await;
    schema_store
        .overwrite_for_tests(
            object_key(UPLOAD_STATE_OBJECT),
            reencrypt_upload_state_value(&opened, UPLOAD_STATE_OBJECT, UPLOAD_STATE, |state| {
                state["schema_version"] = json!(999);
            }),
        )
        .await;
    let schema_error = pipeline
        .read_repository_upload_state(
            &schema_store,
            &opened.master_key,
            UPLOAD_WRITER_ID,
            UPLOAD_ID,
        )
        .await
        .expect_err("unsupported upload state schema is rejected");
    assert!(matches!(
        schema_error,
        CoreError::InvalidUploadState {
            reason: "unsupported upload state schema version",
            ..
        }
    ));

    let format_store = load_upload_state_fixture().await;
    format_store
        .overwrite_for_tests(
            object_key(UPLOAD_STATE_OBJECT),
            reencrypt_upload_state_value(&opened, UPLOAD_STATE_OBJECT, UPLOAD_STATE, |state| {
                state["format_version"] = json!(999);
            }),
        )
        .await;
    let format_error = pipeline
        .read_repository_upload_state(
            &format_store,
            &opened.master_key,
            UPLOAD_WRITER_ID,
            UPLOAD_ID,
        )
        .await
        .expect_err("unsupported upload state format is rejected");
    assert!(matches!(
        format_error,
        CoreError::InvalidUploadState {
            reason: "unsupported upload state format version",
            ..
        }
    ));
}

#[tokio::test]
async fn upload_state_fixture_rejects_stale_repository_state_replay() {
    let store = load_upload_state_fixture().await;
    let opened = open_repository(&store, &secret(UPLOAD_STATE_PASSPHRASE))
        .await
        .expect("upload-state fixture opens");
    store
        .overwrite_for_tests(
            object_key("commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            br#"{"schema_version":0}"#.to_vec(),
        )
        .await;

    let error = upload_state_pipeline()
        .read_repository_upload_state_for_resume(
            &store,
            &opened.master_key,
            UPLOAD_WRITER_ID,
            UPLOAD_ID,
        )
        .await
        .expect_err("stale upload state is rejected before resume");

    assert!(matches!(
        error,
        CoreError::UploadRepositoryStateChanged { .. }
    ));
}

#[tokio::test]
async fn lease_state_fixture_reads_validates_and_rewrites_idempotently() {
    let store = load_lease_state_fixture().await;
    let opened = open_repository(&store, &secret(LEASE_STATE_PASSPHRASE))
        .await
        .expect("lease-state fixture opens");
    assert_eq!(opened.repository_id, LEASE_STATE_REPOSITORY_ID);

    let pipeline = lease_state_pipeline();
    let state = pipeline
        .read_repository_lease_state(&store, &opened.master_key, LEASE_ID)
        .await
        .expect("lease state reads");
    assert_eq!(state.repository_id, LEASE_STATE_REPOSITORY_ID);
    assert_eq!(state.lease_id, LEASE_ID);
    assert_eq!(state.writer_id, LEASE_WRITER_ID);
    assert_eq!(state.command_kind, RepositoryLeaseCommandKind::Prune);
    assert_eq!(state.acquired_at_unix_seconds, 1_779_382_000);
    assert_eq!(state.expires_at_unix_seconds, 1_779_385_600);

    let active = pipeline
        .read_active_repository_lease_state(&store, &opened.master_key, LEASE_ID, 1_779_383_000)
        .await
        .expect("lease is active before expiration");
    assert_eq!(active, state);

    let rewritten = pipeline
        .write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: state.lease_id,
                writer_id: state.writer_id,
                command_kind: state.command_kind,
                acquired_at_unix_seconds: state.acquired_at_unix_seconds,
                expires_at_unix_seconds: state.expires_at_unix_seconds,
            },
        )
        .await
        .expect("same lease state write is idempotent");
    assert_eq!(rewritten.lease_object.as_str(), LEASE_STATE_OBJECT);
    assert!(!rewritten.created);
    assert_eq!(rewritten.bytes_written, 0);
}

#[tokio::test]
async fn lease_state_fixture_rejects_malformed_framing_and_metadata() {
    let store = load_lease_state_fixture().await;
    let opened = open_repository(&store, &secret(LEASE_STATE_PASSPHRASE))
        .await
        .expect("lease-state fixture opens");
    let pipeline = lease_state_pipeline();

    store
        .overwrite_for_tests(object_key(LEASE_STATE_OBJECT), b"not-json".to_vec())
        .await;
    let framing_error = pipeline
        .read_repository_lease_state(&store, &opened.master_key, LEASE_ID)
        .await
        .expect_err("malformed lease state frame is rejected");
    assert!(matches!(framing_error, CoreError::LeaseStateDecode { .. }));

    store
        .overwrite_for_tests(
            object_key(LEASE_STATE_OBJECT),
            reencrypt_lease_state_plaintext(
                &opened,
                LEASE_STATE_OBJECT,
                br#"{"schema_version":"bad"}"#,
            ),
        )
        .await;
    let metadata_error = pipeline
        .read_repository_lease_state(&store, &opened.master_key, LEASE_ID)
        .await
        .expect_err("malformed decrypted lease metadata is rejected");
    assert!(matches!(metadata_error, CoreError::LeaseStateDecode { .. }));
}

#[tokio::test]
async fn lease_state_fixture_rejects_tampered_ciphertext() {
    let opened_store = load_lease_state_fixture().await;
    let opened = open_repository(&opened_store, &secret(LEASE_STATE_PASSPHRASE))
        .await
        .expect("lease-state fixture opens");
    let store = load_lease_state_fixture().await;
    store
        .overwrite_for_tests(
            object_key(LEASE_STATE_OBJECT),
            tamper_fixture_ciphertext(LEASE_STATE),
        )
        .await;

    let error = lease_state_pipeline()
        .read_repository_lease_state(&store, &opened.master_key, LEASE_ID)
        .await
        .expect_err("tampered lease state authentication fails");

    assert!(matches!(
        error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == LEASE_STATE_OBJECT
    ));
}

#[tokio::test]
async fn lease_state_fixture_rejects_wrong_authenticated_name_or_kind() {
    let store = load_lease_state_fixture().await;
    let opened = open_repository(&store, &secret(LEASE_STATE_PASSPHRASE))
        .await
        .expect("lease-state fixture opens");
    let pipeline = lease_state_pipeline();

    let wrong_lease_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let wrong_lease_object =
        "locks/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    store
        .overwrite_for_tests(object_key(wrong_lease_object), LEASE_STATE.to_vec())
        .await;
    let wrong_name_error = pipeline
        .read_repository_lease_state(&store, &opened.master_key, wrong_lease_id)
        .await
        .expect_err("lease state bytes under the wrong object name fail authentication");
    assert!(matches!(
        wrong_name_error,
        CoreError::ObjectAuthentication { key, .. } if key.as_str() == wrong_lease_object
    ));

    let lease_key = opened
        .master_key
        .derive_subkey(KeyPurpose::LeaseState, LEASE_STATE_REPOSITORY_ID.as_bytes())
        .expect("lease state subkey");
    let wrong_kind_context =
        ObjectContext::new(ObjectKind::Index, LEASE_STATE_OBJECT).expect("wrong-kind context");
    let wrong_kind = decrypt_object(
        &lease_key,
        &wrong_kind_context,
        &decode_fixture_encrypted_object(LEASE_STATE),
    );
    assert!(wrong_kind.is_err());
}

#[tokio::test]
async fn lease_state_fixture_rejects_identity_repository_and_expiration_mismatches() {
    let store = load_lease_state_fixture().await;
    let opened = open_repository(&store, &secret(LEASE_STATE_PASSPHRASE))
        .await
        .expect("lease-state fixture opens");

    store
        .overwrite_for_tests(
            object_key(LEASE_STATE_OBJECT),
            reencrypt_lease_state_value(&opened, LEASE_STATE_OBJECT, LEASE_STATE, |state| {
                state["writer_id"] =
                    json!("dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd");
            }),
        )
        .await;
    let identity_error = lease_state_pipeline()
        .read_repository_lease_state(&store, &opened.master_key, LEASE_ID)
        .await
        .expect_err("lease state identity mismatch is rejected");
    assert!(matches!(
        identity_error,
        CoreError::MetadataIdentityMismatch {
            kind: "lease state",
            object_key,
            ..
        } if object_key.as_str() == LEASE_STATE_OBJECT
    ));

    let repository_store = load_lease_state_fixture().await;
    repository_store
        .overwrite_for_tests(
            object_key(LEASE_STATE_OBJECT),
            reencrypt_lease_state_value(&opened, LEASE_STATE_OBJECT, LEASE_STATE, |state| {
                state["repository_id"] =
                    json!("0000000000000000000000000000000000000000000000000000000000000000");
            }),
        )
        .await;
    let repository_error = lease_state_pipeline()
        .read_repository_lease_state(&repository_store, &opened.master_key, LEASE_ID)
        .await
        .expect_err("lease repository mismatch is rejected");
    assert!(matches!(
        repository_error,
        CoreError::InvalidLeaseState {
            reason: "lease state repository id does not match this repository",
            ..
        }
    ));

    let window_store = load_lease_state_fixture().await;
    window_store
        .overwrite_for_tests(
            object_key(LEASE_STATE_OBJECT),
            reencrypt_lease_state_value(&opened, LEASE_STATE_OBJECT, LEASE_STATE, |state| {
                state["expires_at_unix_seconds"] = state["acquired_at_unix_seconds"].clone();
            }),
        )
        .await;
    let window_error = lease_state_pipeline()
        .read_repository_lease_state(&window_store, &opened.master_key, LEASE_ID)
        .await
        .expect_err("lease expiration window is validated");
    assert!(matches!(
        window_error,
        CoreError::InvalidLeaseState {
            reason: "lease expiration must be after acquisition time",
            ..
        }
    ));

    let expired_store = load_lease_state_fixture().await;
    let expired_error = lease_state_pipeline()
        .read_active_repository_lease_state(
            &expired_store,
            &opened.master_key,
            LEASE_ID,
            1_779_385_600,
        )
        .await
        .expect_err("expired lease is rejected for active use");
    assert!(matches!(
        expired_error,
        CoreError::RepositoryLeaseExpired { .. }
    ));
}

#[tokio::test]
async fn lease_state_fixture_rejects_unsupported_versions() {
    let opened_store = load_lease_state_fixture().await;
    let opened = open_repository(&opened_store, &secret(LEASE_STATE_PASSPHRASE))
        .await
        .expect("lease-state fixture opens");
    let pipeline = lease_state_pipeline();

    let schema_store = load_lease_state_fixture().await;
    schema_store
        .overwrite_for_tests(
            object_key(LEASE_STATE_OBJECT),
            reencrypt_lease_state_value(&opened, LEASE_STATE_OBJECT, LEASE_STATE, |state| {
                state["schema_version"] = json!(999);
            }),
        )
        .await;
    let schema_error = pipeline
        .read_repository_lease_state(&schema_store, &opened.master_key, LEASE_ID)
        .await
        .expect_err("unsupported lease schema is rejected");
    assert!(matches!(
        schema_error,
        CoreError::InvalidLeaseState {
            reason: "unsupported lease state schema version",
            ..
        }
    ));

    let format_store = load_lease_state_fixture().await;
    format_store
        .overwrite_for_tests(
            object_key(LEASE_STATE_OBJECT),
            reencrypt_lease_state_value(&opened, LEASE_STATE_OBJECT, LEASE_STATE, |state| {
                state["format_version"] = json!(999);
            }),
        )
        .await;
    let format_error = pipeline
        .read_repository_lease_state(&format_store, &opened.master_key, LEASE_ID)
        .await
        .expect_err("unsupported lease format is rejected");
    assert!(matches!(
        format_error,
        CoreError::InvalidLeaseState {
            reason: "unsupported lease state format version",
            ..
        }
    ));
}

#[tokio::test]
#[ignore = "fixture generation utility; run manually when intentionally updating upload-state bytes"]
async fn regenerate_upload_state_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/repository-format/v0/upload-state");
    let store = FakeObjectStore::new();
    let opened = create_repository(
        &store,
        &secret(UPLOAD_STATE_PASSPHRASE),
        fileferry_crypto::KdfParams::for_tests(),
    )
    .await
    .expect("create upload-state fixture repository")
    .repository;
    let pipeline = BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id.clone()))
        .expect("upload-state fixture pipeline");
    pipeline
        .write_repository_upload_state(
            &store,
            &opened.master_key,
            RepositoryUploadStateRequest {
                writer_id: UPLOAD_WRITER_ID.to_owned(),
                upload_id: UPLOAD_ID.to_owned(),
                created_at_unix_seconds: 1_779_381_000,
                operation: RepositoryUploadOperation::BackupSnapshot,
                pending_objects: vec![
                    RepositoryUploadPendingObject {
                        object_key: "objects/chunk/aa/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                        kind: RepositoryUploadPendingObjectKind::Chunk,
                        bytes: Some(128),
                    },
                    RepositoryUploadPendingObject {
                        object_key: "objects/index/bb/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
                        kind: RepositoryUploadPendingObjectKind::Index,
                        bytes: Some(256),
                    },
                    RepositoryUploadPendingObject {
                        object_key: "objects/manifest/cc/cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc".to_owned(),
                        kind: RepositoryUploadPendingObjectKind::Manifest,
                        bytes: None,
                    },
                ],
            },
        )
        .await
        .expect("write upload-state fixture");

    fs::create_dir_all(root.join("objects/upload").join(UPLOAD_WRITER_ID))
        .expect("create upload-state fixture directories");
    fs::write(
        root.join("bootstrap"),
        store
            .get(&object_key("bootstrap"))
            .await
            .expect("fixture bootstrap bytes"),
    )
    .expect("write fixture bootstrap");
    fs::write(
        root.join(UPLOAD_STATE_OBJECT),
        store
            .get(&object_key(UPLOAD_STATE_OBJECT))
            .await
            .expect("fixture upload-state bytes"),
    )
    .expect("write fixture upload-state object");

    println!("repository_id={}", opened.repository_id);
}

#[tokio::test]
#[ignore = "fixture generation utility; run manually when intentionally updating lease-state bytes"]
async fn regenerate_lease_state_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/repository-format/v0/lease-state");
    let store = FakeObjectStore::new();
    let opened = create_repository(
        &store,
        &secret(LEASE_STATE_PASSPHRASE),
        fileferry_crypto::KdfParams::for_tests(),
    )
    .await
    .expect("create lease-state fixture repository")
    .repository;
    let pipeline = BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id.clone()))
        .expect("lease-state fixture pipeline");
    pipeline
        .write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: LEASE_ID.to_owned(),
                writer_id: LEASE_WRITER_ID.to_owned(),
                command_kind: RepositoryLeaseCommandKind::Prune,
                acquired_at_unix_seconds: 1_779_382_000,
                expires_at_unix_seconds: 1_779_385_600,
            },
        )
        .await
        .expect("write lease-state fixture");

    fs::create_dir_all(root.join("locks")).expect("create lease-state fixture directories");
    fs::write(
        root.join("bootstrap"),
        store
            .get(&object_key("bootstrap"))
            .await
            .expect("fixture bootstrap bytes"),
    )
    .expect("write fixture bootstrap");
    fs::write(
        root.join(LEASE_STATE_OBJECT),
        store
            .get(&object_key(LEASE_STATE_OBJECT))
            .await
            .expect("fixture lease-state bytes"),
    )
    .expect("write fixture lease-state object");

    println!("repository_id={}", opened.repository_id);
}
