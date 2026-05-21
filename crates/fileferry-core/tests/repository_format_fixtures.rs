use fileferry_core::{CoreError, open_repository};
use fileferry_storage::ObjectKey;
use fileferry_testkit::FakeObjectStore;
use secrecy::SecretString;
use serde_json::{Value, json};

const REPOSITORY_ID: &str = "b65c7dfa2394e1b21ebea003397da66b721cc793b376e1edcd78d7f990954771";
const KEY_SLOT_ID: &str = "370e4852331603403cbd038dfa1f4cc4577d2f326f94a19a218be7dc88921f50";
const PRIMARY_PASSPHRASE: &str = "fixture-primary-passphrase-v0";
const ADDED_PASSPHRASE: &str = "fixture-added-passphrase-v0";

const BOOTSTRAP: &[u8] =
    include_bytes!("../../../tests/fixtures/repository-format/v0/bootstrap-keyslot/bootstrap");
const KEY_SLOT: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/bootstrap-keyslot/key-slots/370e4852331603403cbd038dfa1f4cc4577d2f326f94a19a218be7dc88921f50"
);
const KEY_SLOT_REMOVAL: &[u8] = include_bytes!(
    "../../../tests/fixtures/repository-format/v0/bootstrap-keyslot/key-slot-removals/370e4852331603403cbd038dfa1f4cc4577d2f326f94a19a218be7dc88921f50"
);

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
