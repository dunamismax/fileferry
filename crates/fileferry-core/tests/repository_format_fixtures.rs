use fileferry_core::{
    BackupPipeline, BackupPipelineConfig, CheckRepositoryOptions, CoreError,
    RepositoryAeadAlgorithm, RestoreContentRequest, SnapshotManifest, open_repository,
    verify_repository_recovery_export,
};
use fileferry_crypto::{
    AeadAlgorithm, EncryptedObject, KeyPurpose, ObjectContext, ObjectKind, decrypt_object,
    encrypt_object,
};
use fileferry_storage::ObjectKey;
use fileferry_testkit::FakeObjectStore;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

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

fn snapshot_data_pipeline() -> BackupPipeline {
    BackupPipeline::new(BackupPipelineConfig::new(SNAPSHOT_DATA_REPOSITORY_ID))
        .expect("snapshot-data pipeline")
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
