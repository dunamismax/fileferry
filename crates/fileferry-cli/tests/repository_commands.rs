use assert_cmd::Command;
use fileferry_storage::{ObjectKey, ObjectKeyPrefix, ObjectStore, S3Store, S3StoreConfig};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::Output,
    time::{Duration, SystemTime},
};

fn fileferry() -> Command {
    let mut command = Command::cargo_bin("ferry").expect("ferry binary");
    for variable in [
        "FILEFERRY_CONFIG",
        "FILEFERRY_PROFILE",
        "FILEFERRY_REPOSITORY",
        "FILEFERRY_PASSWORD",
        "FILEFERRY_PASSWORD_FILE",
        "FILEFERRY_NEW_PASSWORD",
        "FILEFERRY_NEW_PASSWORD_FILE",
        "FILEFERRY_S3_ENDPOINT",
        "FILEFERRY_S3_REGION",
        "FILEFERRY_S3_ACCESS_KEY_ID",
        "FILEFERRY_S3_SECRET_ACCESS_KEY",
        "FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE",
        "FILEFERRY_LOG",
    ] {
        command.env_remove(variable);
    }
    command
}

fn init_repo(repo_url: &str, passphrase: &str) {
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", repo_url, "init"])
        .assert()
        .success()
        .stderr("");
}

fn backup_source(repo_url: &str, passphrase: &str, source: &std::path::Path) -> Value {
    backup_source_with_tags(repo_url, passphrase, source, &["cli"])
}

fn backup_source_with_tags(
    repo_url: &str,
    passphrase: &str,
    source: &std::path::Path,
    tags: &[&str],
) -> Value {
    let mut args = vec!["--repo", repo_url, "--json", "backup"];
    for tag in tags {
        args.push("--tag");
        args.push(tag);
    }
    args.push(source.to_str().expect("source path"));

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(args)
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();

    serde_json::from_slice(&output).expect("backup json")
}

fn file_count_under(path: &Path) -> usize {
    if !path.exists() {
        return 0;
    }

    let mut pending = vec![path.to_path_buf()];
    let mut count = 0;
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).expect("read directory") {
            let entry = entry.expect("directory entry");
            let file_type = entry.file_type().expect("entry type");
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file() {
                count += 1;
            }
        }
    }

    count
}

fn expected_restore_metadata_fields(entries_with_file_or_directory_metadata: usize) -> usize {
    if cfg!(unix) {
        entries_with_file_or_directory_metadata * 4
    } else {
        entries_with_file_or_directory_metadata
    }
}

fn set_modified_time(path: &Path, modified: SystemTime) {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open file for timestamp update");
    file.set_times(fs::FileTimes::new().set_modified(modified))
        .expect("set file modified time");
}

fn patterned_bytes(seed: usize, len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| ((index * 29 + seed * 11 + index / 3) % 251) as u8)
        .collect()
}

#[test]
fn init_creates_encrypted_local_repository_and_snapshots_lists_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";

    let init_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "init"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let init: Value = serde_json::from_slice(&init_output).expect("init json");
    assert_eq!(init["command"], "init");
    assert_eq!(init["status"], "success");
    assert_eq!(init["data"]["backend"], "local");
    assert_eq!(init["data"]["created"], true);
    assert_eq!(init["data"]["format_version"], 0);
    assert_eq!(init["data"]["key_slots"], 1);
    assert!(repo.join("bootstrap").is_file());

    let empty_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let empty: Value = serde_json::from_slice(&empty_output).expect("snapshots json");
    assert_eq!(
        empty["data"]["snapshots"]
            .as_array()
            .expect("snapshot array")
            .len(),
        0
    );
}

#[test]
fn key_add_adds_passphrase_slot_and_new_passphrase_unlocks_repository() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let added_passphrase = "added-passphrase";
    init_repo(&repo_url, passphrase);
    let bootstrap_before = fs::read(repo.join("bootstrap")).expect("bootstrap before");

    let key_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let key_text = String::from_utf8(key_output.clone()).expect("key add utf8");
    let key: Value = serde_json::from_slice(&key_output).expect("key add json");
    assert_eq!(key["command"], "key add");
    assert_eq!(key["status"], "success");
    assert_eq!(key["data"]["key_slots"], 2);
    assert_eq!(key["data"]["reencrypted_repository_objects"], false);
    assert_eq!(key["data"]["kdf"]["algorithm"], "argon2id_v19");
    assert!(
        !key["data"]["key_slot_id"]
            .as_str()
            .expect("slot id")
            .is_empty()
    );
    assert!(!key_text.contains(passphrase));
    assert!(!key_text.contains(added_passphrase));
    assert_eq!(
        fs::read(repo.join("bootstrap")).expect("bootstrap after"),
        bootstrap_before
    );
    assert_eq!(file_count_under(&repo.join("key-slots")), 1);

    let snapshots_output = fileferry()
        .env("FILEFERRY_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(snapshots["status"], "success");
}

#[test]
fn key_add_supports_jsonl_and_new_password_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let new_password_file = temp.path().join("new-password.txt");
    let passphrase = "test-passphrase";
    fs::write(&new_password_file, "added-passphrase\n").expect("write new password file");
    init_repo(&repo_url, passphrase);

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "key",
            "add",
            "--new-password-file",
            new_password_file.to_str().expect("new password file path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let started: Value = serde_json::from_slice(lines[0]).expect("started event");
    let completed: Value = serde_json::from_slice(lines[1]).expect("completed event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "key add");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["command"], "key add");
    assert_eq!(completed["data"]["key_slots"], 2);
}

#[test]
fn key_add_failures_are_structured_redacted_and_mapped_to_exit_codes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let added_passphrase = "added-passphrase";
    init_repo(&repo_url, passphrase);

    let missing_new_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing_new: Value =
        serde_json::from_slice(&missing_new_output).expect("missing new password json");
    assert_eq!(
        missing_new["data"]["code"],
        "repository_new_password_missing"
    );
    assert_eq!(missing_new["data"]["exit_code"], 2);

    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-current-passphrase")
        .env("FILEFERRY_NEW_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_text = String::from_utf8(wrong_output.clone()).expect("wrong utf8");
    let wrong: Value = serde_json::from_slice(&wrong_output).expect("wrong json");
    assert_eq!(wrong["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong["data"]["exit_code"], 4);
    assert!(!wrong_text.contains("wrong-current-passphrase"));
    assert!(!wrong_text.contains(added_passphrase));
    assert_eq!(file_count_under(&repo.join("key-slots")), 0);

    let malformed_slot =
        repo.join("key-slots/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    fs::create_dir_all(malformed_slot.parent().expect("slot parent")).expect("create key slots");
    fs::write(&malformed_slot, b"not-json").expect("write malformed key slot");
    let malformed_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let malformed: Value = serde_json::from_slice(&malformed_output).expect("malformed json");
    assert_eq!(
        malformed["data"]["code"],
        "repository_key_slot_decode_failed"
    );
    assert_eq!(malformed["data"]["exit_code"], 6);
    assert_eq!(
        malformed["data"]["object_key"],
        "key-slots/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
}

#[test]
fn key_remove_marks_added_slot_without_deleting_key_slot_object() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let added_passphrase = "added-passphrase";
    init_repo(&repo_url, passphrase);
    let add_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let add: Value = serde_json::from_slice(&add_output).expect("key add json");
    let key_slot_id = add["data"]["key_slot_id"].as_str().expect("slot id");

    let remove_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "key", "remove", key_slot_id])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let remove_text = String::from_utf8(remove_output.clone()).expect("key remove utf8");
    let remove: Value = serde_json::from_slice(&remove_output).expect("key remove json");
    assert_eq!(remove["command"], "key remove");
    assert_eq!(remove["status"], "success");
    assert_eq!(remove["data"]["removed_key_slot_id"], key_slot_id);
    assert_eq!(remove["data"]["key_slots"], 1);
    assert_eq!(
        remove["data"]["removal_marker_object"],
        format!("key-slot-removals/{key_slot_id}")
    );
    assert_eq!(remove["data"]["removal_marker_created"], true);
    assert_eq!(remove["data"]["deleted_key_slot_objects"], false);
    assert_eq!(remove["data"]["reencrypted_repository_objects"], false);
    assert!(!remove_text.contains(passphrase));
    assert!(!remove_text.contains(added_passphrase));
    assert_eq!(file_count_under(&repo.join("key-slots")), 1);
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 1);

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("");

    let removed_unlock = fileferry()
        .env("FILEFERRY_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let removed_unlock: Value =
        serde_json::from_slice(&removed_unlock).expect("removed unlock json");
    assert_eq!(removed_unlock["data"]["code"], "repository_unlock_failed");
}

#[test]
fn key_remove_supports_jsonl_and_is_idempotent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let added_passphrase = "added-passphrase";
    init_repo(&repo_url, passphrase);
    let add_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let add: Value = serde_json::from_slice(&add_output).expect("key add json");
    let key_slot_id = add["data"]["key_slot_id"].as_str().expect("slot id");

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "key", "remove", key_slot_id])
        .assert()
        .success()
        .stdout(predicates::str::contains("deleted_key_slot_objects=false"))
        .stderr("");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "key", "remove", key_slot_id])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let started: Value = serde_json::from_slice(lines[0]).expect("started event");
    let completed: Value = serde_json::from_slice(lines[1]).expect("completed event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "key remove");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["command"], "key remove");
    assert_eq!(completed["data"]["key_slots"], 1);
    assert_eq!(completed["data"]["removal_marker_created"], false);
}

#[test]
fn key_remove_failures_are_structured_redacted_and_mapped_to_exit_codes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let added_passphrase = "added-passphrase";
    init_repo(&repo_url, passphrase);
    let add_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let add: Value = serde_json::from_slice(&add_output).expect("key add json");
    let key_slot_id = add["data"]["key_slot_id"]
        .as_str()
        .expect("slot id")
        .to_owned();

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "key", "remove", "not-a-slot-id"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains(
            "key slot id must be 64 hexadecimal characters",
        ));

    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-current-passphrase")
        .args(["--repo", &repo_url, "--json", "key", "remove", &key_slot_id])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_text = String::from_utf8(wrong_output.clone()).expect("wrong utf8");
    let wrong: Value = serde_json::from_slice(&wrong_output).expect("wrong json");
    assert_eq!(wrong["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong["data"]["exit_code"], 4);
    assert!(!wrong_text.contains("wrong-current-passphrase"));
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 0);

    let lockout_output = fileferry()
        .env("FILEFERRY_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "remove", &key_slot_id])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lockout_text = String::from_utf8(lockout_output.clone()).expect("lockout utf8");
    let lockout: Value = serde_json::from_slice(&lockout_output).expect("lockout json");
    assert_eq!(
        lockout["data"]["code"],
        "repository_key_slot_removal_would_lock_out"
    );
    assert_eq!(lockout["data"]["exit_code"], 4);
    assert!(!lockout_text.contains(added_passphrase));
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 0);

    let missing = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let missing_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "key", "remove", missing])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing: Value = serde_json::from_slice(&missing_output).expect("missing json");
    assert_eq!(missing["data"]["code"], "repository_key_slot_not_found");
    assert_eq!(missing["data"]["exit_code"], 7);

    let malformed_marker = repo.join(format!("key-slot-removals/{key_slot_id}"));
    fs::create_dir_all(malformed_marker.parent().expect("marker parent"))
        .expect("create key-slot-removals");
    fs::write(&malformed_marker, b"not-json").expect("write malformed marker");
    let malformed_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "key", "remove", &key_slot_id])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let malformed: Value = serde_json::from_slice(&malformed_output).expect("malformed json");
    assert_eq!(
        malformed["data"]["code"],
        "repository_key_slot_removal_decode_failed"
    );
    assert_eq!(malformed["data"]["exit_code"], 6);
    assert_eq!(
        malformed["data"]["object_key"],
        format!("key-slot-removals/{key_slot_id}")
    );
}

#[test]
fn key_rotate_adds_new_slot_and_retires_selected_current_slot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let old_passphrase = "old-passphrase";
    let new_passphrase = "new-passphrase";
    init_repo(&repo_url, passphrase);
    let bootstrap_before = fs::read(repo.join("bootstrap")).expect("bootstrap before");
    let add_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", old_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let add: Value = serde_json::from_slice(&add_output).expect("key add json");
    let old_key_slot_id = add["data"]["key_slot_id"].as_str().expect("slot id");

    let rotate_output = fileferry()
        .env("FILEFERRY_PASSWORD", old_passphrase)
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "rotate",
            "--retire-key-slot",
            old_key_slot_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let rotate_text = String::from_utf8(rotate_output.clone()).expect("key rotate utf8");
    let rotate: Value = serde_json::from_slice(&rotate_output).expect("key rotate json");
    assert_eq!(rotate["command"], "key rotate");
    assert_eq!(rotate["status"], "success");
    assert_eq!(
        rotate["data"]["removed_key_slot_ids"],
        json!([old_key_slot_id])
    );
    assert_eq!(rotate["data"]["key_slots"], 2);
    assert_eq!(
        rotate["data"]["removal_marker_objects"],
        json!([format!("key-slot-removals/{old_key_slot_id}")])
    );
    assert_eq!(rotate["data"]["removal_markers_created"], 1);
    assert_eq!(rotate["data"]["deleted_key_slot_objects"], false);
    assert_eq!(rotate["data"]["reencrypted_repository_objects"], false);
    assert_eq!(rotate["data"]["kdf"]["algorithm"], "argon2id_v19");
    assert_ne!(
        rotate["data"]["added_key_slot_id"]
            .as_str()
            .expect("new slot id"),
        old_key_slot_id
    );
    assert!(!rotate_text.contains(passphrase));
    assert!(!rotate_text.contains(old_passphrase));
    assert!(!rotate_text.contains(new_passphrase));
    assert_eq!(
        fs::read(repo.join("bootstrap")).expect("bootstrap after"),
        bootstrap_before
    );
    assert_eq!(file_count_under(&repo.join("key-slots")), 2);
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 1);

    fileferry()
        .env("FILEFERRY_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("");
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("");

    let old_unlock = fileferry()
        .env("FILEFERRY_PASSWORD", old_passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let old_unlock: Value = serde_json::from_slice(&old_unlock).expect("old unlock json");
    assert_eq!(old_unlock["data"]["code"], "repository_unlock_failed");
}

#[test]
fn key_rotate_supports_jsonl_and_new_password_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let new_password_file = temp.path().join("new-password.txt");
    let passphrase = "test-passphrase";
    let old_passphrase = "old-passphrase";
    fs::write(&new_password_file, "new-passphrase\n").expect("write new password file");
    init_repo(&repo_url, passphrase);
    let add_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", old_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let add: Value = serde_json::from_slice(&add_output).expect("key add json");
    let old_key_slot_id = add["data"]["key_slot_id"].as_str().expect("slot id");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "key",
            "rotate",
            "--new-password-file",
            new_password_file.to_str().expect("new password file path"),
            "--retire-key-slot",
            old_key_slot_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let started: Value = serde_json::from_slice(lines[0]).expect("started event");
    let completed: Value = serde_json::from_slice(lines[1]).expect("completed event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "key rotate");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["command"], "key rotate");
    assert_eq!(completed["data"]["key_slots"], 2);
    assert_eq!(completed["data"]["removal_markers_created"], 1);
}

#[test]
fn key_rotate_failures_are_structured_redacted_and_mapped_to_exit_codes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let old_passphrase = "old-passphrase";
    let new_passphrase = "new-passphrase";
    init_repo(&repo_url, passphrase);
    let add_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", old_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let add: Value = serde_json::from_slice(&add_output).expect("key add json");
    let old_key_slot_id = add["data"]["key_slot_id"]
        .as_str()
        .expect("slot id")
        .to_owned();

    let missing_new_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "rotate",
            "--retire-key-slot",
            &old_key_slot_id,
        ])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing_new: Value =
        serde_json::from_slice(&missing_new_output).expect("missing new password json");
    assert_eq!(
        missing_new["data"]["code"],
        "repository_new_password_missing"
    );
    assert_eq!(missing_new["data"]["exit_code"], 2);

    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-current-passphrase")
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "rotate",
            "--retire-key-slot",
            &old_key_slot_id,
        ])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_text = String::from_utf8(wrong_output.clone()).expect("wrong utf8");
    let wrong: Value = serde_json::from_slice(&wrong_output).expect("wrong json");
    assert_eq!(wrong["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong["data"]["exit_code"], 4);
    assert!(!wrong_text.contains("wrong-current-passphrase"));
    assert!(!wrong_text.contains(new_passphrase));
    assert_eq!(file_count_under(&repo.join("key-slots")), 1);
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 0);

    let missing = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let missing_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "rotate",
            "--retire-key-slot",
            missing,
        ])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing: Value = serde_json::from_slice(&missing_output).expect("missing json");
    assert_eq!(missing["data"]["code"], "repository_key_slot_not_found");
    assert_eq!(missing["data"]["exit_code"], 7);
    assert_eq!(file_count_under(&repo.join("key-slots")), 1);
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 0);

    fs::write(
        repo.join(format!("key-slots/{old_key_slot_id}")),
        b"not-json",
    )
    .expect("corrupt old key slot");
    let malformed_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "rotate",
            "--retire-key-slot",
            &old_key_slot_id,
        ])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let malformed: Value = serde_json::from_slice(&malformed_output).expect("malformed json");
    assert_eq!(
        malformed["data"]["code"],
        "repository_key_slot_decode_failed"
    );
    assert_eq!(malformed["data"]["exit_code"], 6);
    assert_eq!(
        malformed["data"]["object_key"],
        format!("key-slots/{old_key_slot_id}")
    );
    assert_eq!(file_count_under(&repo.join("key-slots")), 1);
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 0);
}

#[test]
fn key_rotate_reports_malformed_removal_marker_before_writing_new_slot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let old_passphrase = "old-passphrase";
    let new_passphrase = "new-passphrase";
    init_repo(&repo_url, passphrase);
    let add_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", old_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let add: Value = serde_json::from_slice(&add_output).expect("key add json");
    let old_key_slot_id = add["data"]["key_slot_id"].as_str().expect("slot id");
    let malformed_marker = repo.join(format!("key-slot-removals/{old_key_slot_id}"));
    fs::create_dir_all(malformed_marker.parent().expect("marker parent"))
        .expect("create key-slot-removals");
    fs::write(&malformed_marker, b"not-json").expect("write malformed marker");

    let malformed_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "rotate",
            "--retire-key-slot",
            old_key_slot_id,
        ])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let malformed: Value = serde_json::from_slice(&malformed_output).expect("malformed json");
    assert_eq!(
        malformed["data"]["code"],
        "repository_key_slot_removal_decode_failed"
    );
    assert_eq!(malformed["data"]["exit_code"], 6);
    assert_eq!(
        malformed["data"]["object_key"],
        format!("key-slot-removals/{old_key_slot_id}")
    );
    assert_eq!(file_count_under(&repo.join("key-slots")), 1);
}

#[test]
fn key_export_recovery_writes_encrypted_package_without_leaking_secrets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let export_path = temp.path().join("recovery.fileferry-key");
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);
    let bootstrap_before = fs::read(repo.join("bootstrap")).expect("bootstrap before");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "export-recovery",
            "--output",
            export_path.to_str().expect("export path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let output_text = String::from_utf8(output.clone()).expect("export utf8");
    let exported: Value = serde_json::from_slice(&output).expect("export json");
    assert_eq!(exported["command"], "key export-recovery");
    assert_eq!(exported["status"], "success");
    assert_eq!(exported["data"]["key_slots"], 1);
    assert_eq!(exported["data"]["kdf"]["algorithm"], "argon2id_v19");
    assert_eq!(exported["data"]["aead"], "xchacha20_poly1305");
    assert_eq!(exported["data"]["recovery_import_implemented"], false);
    assert_eq!(exported["data"]["raw_master_key_exported"], false);
    assert_eq!(exported["data"]["reencrypted_repository_objects"], false);
    assert!(export_path.is_file());
    assert!(!output_text.contains(passphrase));
    assert!(!output_text.contains(&repo_url));
    assert_eq!(
        fs::read(repo.join("bootstrap")).expect("bootstrap after"),
        bootstrap_before
    );
    assert_eq!(file_count_under(&repo.join("key-slots")), 0);

    let export_bytes = fs::read(&export_path).expect("read export");
    let export_text = String::from_utf8(export_bytes).expect("export file utf8");
    let export_json: Value = serde_json::from_str(&export_text).expect("export file json");
    assert_eq!(export_json["schema_version"], 0);
    assert_eq!(export_json["magic"], "fileferry");
    assert_eq!(export_json["format_version"], 0);
    assert_eq!(export_json["export_type"], "fileferry_recovery_export");
    assert_eq!(
        export_json["repository_id"],
        exported["data"]["repository_id"]
    );
    assert_eq!(export_json["export_id"], exported["data"]["export_id"]);
    assert_eq!(export_json["aead"], "xchacha20_poly1305");
    assert!(export_json["recovery_key"]["wrapped_master_key"].is_array());
    assert!(export_json["master_key_check"].is_string());
    assert!(!export_text.contains(passphrase));
}

#[test]
fn key_export_recovery_supports_jsonl() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let export_path = temp.path().join("recovery.fileferry-key");
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "key",
            "export-recovery",
            "--output",
            export_path.to_str().expect("export path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let started: Value = serde_json::from_slice(lines[0]).expect("started event");
    let completed: Value = serde_json::from_slice(lines[1]).expect("completed event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "key export-recovery");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["command"], "key export-recovery");
    assert_eq!(completed["data"]["raw_master_key_exported"], false);
    assert!(export_path.is_file());
}

#[test]
fn key_export_recovery_failures_are_structured_and_do_not_create_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let wrong_path = temp.path().join("wrong.fileferry-key");
    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-current-passphrase")
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "export-recovery",
            "--output",
            wrong_path.to_str().expect("wrong export path"),
        ])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_text = String::from_utf8(wrong_output.clone()).expect("wrong utf8");
    let wrong: Value = serde_json::from_slice(&wrong_output).expect("wrong json");
    assert_eq!(wrong["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong["data"]["exit_code"], 4);
    assert!(!wrong_text.contains("wrong-current-passphrase"));
    assert!(!wrong_path.exists());

    let existing_path = temp.path().join("existing.fileferry-key");
    fs::write(&existing_path, b"do not overwrite").expect("write existing export");
    let existing_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "export-recovery",
            "--output",
            existing_path.to_str().expect("existing export path"),
        ])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let existing: Value = serde_json::from_slice(&existing_output).expect("existing json");
    assert_eq!(existing["data"]["code"], "recovery_output_exists");
    assert_eq!(existing["data"]["exit_code"], 2);
    assert_eq!(
        fs::read(&existing_path).expect("existing export"),
        b"do not overwrite"
    );

    let malformed_slot =
        repo.join("key-slots/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    fs::create_dir_all(malformed_slot.parent().expect("slot parent")).expect("create key slots");
    fs::write(&malformed_slot, b"not-json").expect("write malformed key slot");
    let malformed_path = temp.path().join("malformed.fileferry-key");
    let malformed_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "export-recovery",
            "--output",
            malformed_path.to_str().expect("malformed export path"),
        ])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let malformed: Value = serde_json::from_slice(&malformed_output).expect("malformed json");
    assert_eq!(
        malformed["data"]["code"],
        "repository_key_slot_decode_failed"
    );
    assert_eq!(malformed["data"]["exit_code"], 6);
    assert_eq!(
        malformed["data"]["object_key"],
        "key-slots/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert!(!malformed_path.exists());
}

#[test]
fn forget_dry_run_reports_plan_without_writing_markers() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let first_source = temp.path().join("first-source");
    let second_source = temp.path().join("second-source");
    fs::create_dir(&first_source).expect("create first source");
    fs::create_dir(&second_source).expect("create second source");
    fs::write(first_source.join("first.txt"), b"first").expect("write first");
    fs::write(second_source.join("second.txt"), b"second").expect("write second");
    backup_source_with_tags(&repo_url, passphrase, &first_source, &["old"]);
    backup_source_with_tags(&repo_url, passphrase, &second_source, &["new"]);

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--dry-run",
            "--keep-last",
            "1",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let forget: Value = serde_json::from_slice(&output).expect("forget json");
    assert_eq!(forget["command"], "forget");
    assert_eq!(forget["status"], "success");
    assert_eq!(forget["data"]["dry_run"], true);
    assert_eq!(forget["data"]["snapshots_matched"], 2);
    assert_eq!(forget["data"]["snapshots_forgotten"], 1);
    assert_eq!(forget["data"]["retained_snapshots"], 1);
    assert_eq!(forget["data"]["object_deletion"], false);
    assert_eq!(forget["data"]["marker_objects_written"], 0);
    assert_eq!(
        forget["data"]["candidate_snapshots"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        forget["data"]["kept_snapshots"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        forget["data"]["forgotten_snapshots"][0]["reasons"],
        serde_json::json!(["not-matched-by-keep-rule"])
    );
    assert_eq!(file_count_under(&repo.join("forgets")), 0);

    let snapshots_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(snapshots["data"]["snapshots"].as_array().unwrap().len(), 2);
}

#[test]
fn forget_keep_tag_writes_marker_and_does_not_delete_repository_objects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let keep_source = temp.path().join("keep-source");
    let drop_source = temp.path().join("drop-source");
    fs::create_dir(&keep_source).expect("create keep source");
    fs::create_dir(&drop_source).expect("create drop source");
    fs::write(keep_source.join("keep.txt"), b"keep").expect("write keep");
    fs::write(drop_source.join("drop.txt"), b"drop").expect("write drop");
    backup_source_with_tags(&repo_url, passphrase, &keep_source, &["keep"]);
    backup_source_with_tags(&repo_url, passphrase, &drop_source, &["drop"]);
    let commits_before = file_count_under(&repo.join("commits"));
    let manifests_before = file_count_under(&repo.join("objects").join("manifest"));
    let indexes_before = file_count_under(&repo.join("objects").join("index"));
    let chunks_before = file_count_under(&repo.join("objects").join("chunk"));

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--keep-tag",
            "keep",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let forget: Value = serde_json::from_slice(&output).expect("forget json");
    assert_eq!(forget["data"]["dry_run"], false);
    assert_eq!(forget["data"]["snapshots_forgotten"], 1);
    assert_eq!(forget["data"]["retained_snapshots"], 1);
    assert_eq!(forget["data"]["marker_objects_written"], 1);
    assert_eq!(
        forget["data"]["kept_snapshots"][0]["reasons"],
        serde_json::json!(["keep-tag:keep"])
    );
    assert!(
        forget["data"]["forgotten_snapshots"][0]["marker_object"]
            .as_str()
            .expect("marker object")
            .starts_with("forgets/")
    );
    assert_eq!(file_count_under(&repo.join("forgets")), 1);
    assert_eq!(file_count_under(&repo.join("commits")), commits_before);
    assert_eq!(
        file_count_under(&repo.join("objects").join("manifest")),
        manifests_before
    );
    assert_eq!(
        file_count_under(&repo.join("objects").join("index")),
        indexes_before
    );
    assert_eq!(
        file_count_under(&repo.join("objects").join("chunk")),
        chunks_before
    );

    let snapshots_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(snapshots["data"]["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(
        snapshots["data"]["snapshots"][0]["tags"],
        serde_json::json!(["keep"])
    );
}

#[test]
fn forget_jsonl_reports_progress_and_completion_envelope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let first_source = temp.path().join("first-source");
    let second_source = temp.path().join("second-source");
    fs::create_dir(&first_source).expect("create first source");
    fs::create_dir(&second_source).expect("create second source");
    fs::write(first_source.join("first.txt"), b"first").expect("write first");
    fs::write(second_source.join("second.txt"), b"second").expect("write second");
    backup_source_with_tags(&repo_url, passphrase, &first_source, &["first"]);
    backup_source_with_tags(&repo_url, passphrase, &second_source, &["second"]);

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "forget", "--keep-last", "1"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let events = String::from_utf8(output)
        .expect("jsonl utf8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("jsonl event"))
        .collect::<Vec<_>>();

    assert_eq!(events.first().unwrap()["event"], "command_started");
    assert_eq!(events.last().unwrap()["event"], "command_completed");
    assert_eq!(events.last().unwrap()["command"], "forget");
    assert_eq!(events.last().unwrap()["data"]["snapshots_forgotten"], 1);
    assert!(
        events
            .iter()
            .any(|event| event["data"]["phase"] == "write_forget_state")
    );
}

#[test]
fn prune_dry_run_and_sweep_reclaim_forgotten_local_snapshot_objects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let keep_source = temp.path().join("keep-source");
    let drop_source = temp.path().join("drop-source");
    fs::create_dir(&keep_source).expect("create keep source");
    fs::create_dir(&drop_source).expect("create drop source");
    fs::write(keep_source.join("keep.txt"), patterned_bytes(1, 4096)).expect("write keep");
    fs::write(drop_source.join("drop.txt"), patterned_bytes(2, 4096)).expect("write drop");
    backup_source_with_tags(&repo_url, passphrase, &keep_source, &["keep"]);
    backup_source_with_tags(&repo_url, passphrase, &drop_source, &["drop"]);

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "forget", "--keep-tag", "keep"])
        .assert()
        .success()
        .stderr("");
    let objects_before_dry_run = file_count_under(&repo);

    let dry_run_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "prune", "--dry-run"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let dry_run: Value = serde_json::from_slice(&dry_run_output).expect("prune dry-run json");
    assert_eq!(dry_run["command"], "prune");
    assert_eq!(dry_run["status"], "success");
    assert_eq!(dry_run["data"]["dry_run"], true);
    assert_eq!(dry_run["data"]["completed"], true);
    assert_eq!(dry_run["data"]["recovery_state"], "dry_run");
    assert!(
        dry_run["data"]["candidate_object_count"]
            .as_u64()
            .expect("candidate count")
            >= 4
    );
    assert_eq!(dry_run["data"]["deleted_object_count"], 0);
    assert_eq!(file_count_under(&repo), objects_before_dry_run);
    assert_eq!(
        file_count_under(&repo.join("objects").join("prune-plan")),
        0
    );

    let prune_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "prune"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let prune: Value = serde_json::from_slice(&prune_output).expect("prune json");
    assert_eq!(prune["data"]["dry_run"], false);
    assert_eq!(prune["data"]["completed"], true);
    assert_eq!(prune["data"]["recovery_state"], "completed");
    assert_eq!(
        prune["data"]["deleted_object_count"],
        prune["data"]["candidate_object_count"]
    );
    assert!(
        prune["data"]["candidate_objects"]
            .as_array()
            .expect("candidate objects")
            .iter()
            .any(|object| object["kind"] == "commit")
    );
    assert!(file_count_under(&repo) < objects_before_dry_run);
    assert_eq!(
        file_count_under(&repo.join("objects").join("prune-plan")),
        1
    );
    assert_eq!(
        file_count_under(&repo.join("objects").join("prune-completion")),
        1
    );

    let snapshots_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(snapshots["data"]["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(
        snapshots["data"]["snapshots"][0]["tags"],
        serde_json::json!(["keep"])
    );
}

#[test]
fn prune_jsonl_reports_progress_and_completion_envelope() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let first_source = temp.path().join("first-source");
    let second_source = temp.path().join("second-source");
    fs::create_dir(&first_source).expect("create first source");
    fs::create_dir(&second_source).expect("create second source");
    fs::write(first_source.join("first.txt"), b"first").expect("write first");
    fs::write(second_source.join("second.txt"), b"second").expect("write second");
    backup_source_with_tags(&repo_url, passphrase, &first_source, &["first"]);
    backup_source_with_tags(&repo_url, passphrase, &second_source, &["second"]);
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "forget", "--keep-last", "1"])
        .assert()
        .success()
        .stderr("");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "prune"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let events = String::from_utf8(output)
        .expect("jsonl utf8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("jsonl event"))
        .collect::<Vec<_>>();

    assert_eq!(events.first().unwrap()["event"], "command_started");
    assert_eq!(events.last().unwrap()["event"], "command_completed");
    assert_eq!(events.last().unwrap()["command"], "prune");
    assert_eq!(events.last().unwrap()["data"]["completed"], true);
    assert!(events.iter().any(|event| event["data"]["phase"] == "mark"));
    assert!(events.iter().any(|event| event["data"]["phase"] == "sweep"));
}

#[test]
fn prune_malformed_state_has_stable_integrity_exit_code() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let malformed_path = repo
        .join("objects")
        .join("prune-plan")
        .join("aa")
        .join("aa".repeat(32));
    fs::create_dir_all(malformed_path.parent().expect("malformed parent"))
        .expect("create malformed parent");
    fs::write(&malformed_path, b"not encrypted json").expect("write malformed prune plan");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "prune"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failed: Value = serde_json::from_slice(&output).expect("failure json");
    assert_eq!(failed["status"], "failure");
    assert_eq!(
        failed["data"]["code"],
        "repository_prune_plan_decode_failed"
    );
    assert_eq!(failed["data"]["exit_code"], 6);
    assert_eq!(
        failed["data"]["object_key"],
        "objects/prune-plan/aa/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
}

#[test]
fn forget_no_match_and_invalid_policy_have_stable_exit_codes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let no_match_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "forget", "--keep-last", "1"])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let no_match: Value = serde_json::from_slice(&no_match_output).expect("no-match json");
    assert_eq!(no_match["status"], "failure");
    assert_eq!(no_match["data"]["code"], "forget_no_snapshots_matched");
    assert_eq!(no_match["data"]["exit_code"], 7);

    let invalid_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "forget", "--keep-last", "0"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let invalid: Value = serde_json::from_slice(&invalid_output).expect("invalid policy json");
    assert_eq!(invalid["status"], "failure");
    assert_eq!(invalid["data"]["code"], "retention_policy_count_invalid");
    assert_eq!(invalid["data"]["exit_code"], 2);

    let invalid_tag_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--keep-tag",
            "bad,tag",
        ])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let invalid_tag: Value = serde_json::from_slice(&invalid_tag_output).expect("invalid tag json");
    assert_eq!(invalid_tag["status"], "failure");
    assert_eq!(invalid_tag["data"]["code"], "retention_policy_tag_invalid");
    assert_eq!(invalid_tag["data"]["exit_code"], 2);
}

#[test]
fn restore_writes_file_bytes_from_committed_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    fs::create_dir(source.join("nested")).expect("create nested");
    fs::write(source.join("nested").join("keep.txt"), b"keep").expect("write nested");
    let keep_modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    set_modified_time(&source.join("nested").join("keep.txt"), keep_modified);
    let backup = backup_source(&repo_url, passphrase, &source);

    let destination = temp.path().join("restore-tag");
    let restore_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "restore",
            "--tag",
            "cli",
            "--path",
            "nested/keep.txt",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let restore: Value = serde_json::from_slice(&restore_output).expect("restore json");
    assert_eq!(restore["command"], "restore");
    assert_eq!(restore["status"], "success");
    assert_eq!(
        restore["data"]["snapshot_id"],
        backup["data"]["snapshot_id"]
    );
    assert_eq!(
        restore["data"]["paths"],
        serde_json::json!(["nested/keep.txt"])
    );
    assert_eq!(restore["data"]["dry_run"], false);
    assert_eq!(restore["data"]["overwrite"], "fail_if_exists");
    assert_eq!(restore["data"]["entries_selected"], 1);
    assert_eq!(restore["data"]["files_written"], 1);
    assert_eq!(restore["data"]["directories_written"], 0);
    assert_eq!(restore["data"]["symlinks_written"], 0);
    assert_eq!(
        restore["data"]["metadata_planned"],
        expected_restore_metadata_fields(1)
    );
    assert_eq!(
        restore["data"]["metadata_applied"],
        expected_restore_metadata_fields(1)
    );
    assert_eq!(restore["data"]["metadata_warnings"], serde_json::json!([]));
    assert_eq!(restore["data"]["bytes_written"], 4);
    assert_eq!(restore["data"]["verified_files"], 1);
    assert_eq!(
        fs::read(destination.join("nested").join("keep.txt")).expect("restored nested file"),
        b"keep"
    );
    assert_eq!(
        fs::metadata(destination.join("nested").join("keep.txt"))
            .expect("restored nested metadata")
            .modified()
            .expect("restored nested modified time"),
        keep_modified
    );
    assert!(!destination.join("sample.txt").exists());

    let snapshot_id = backup["data"]["snapshot_id"].as_str().expect("snapshot id");
    let dry_run_destination = temp.path().join("restore-dry-run");
    let dry_run_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "restore",
            "--snapshot",
            snapshot_id,
            "--dry-run",
            dry_run_destination
                .to_str()
                .expect("dry run destination path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let dry_run: Value = serde_json::from_slice(&dry_run_output).expect("dry-run json");
    assert_eq!(
        dry_run["data"]["snapshot_id"],
        backup["data"]["snapshot_id"]
    );
    assert_eq!(dry_run["data"]["dry_run"], true);
    assert_eq!(
        dry_run["data"]["metadata_planned"],
        expected_restore_metadata_fields(4)
    );
    assert_eq!(dry_run["data"]["metadata_applied"], 0);
    assert_eq!(dry_run["data"]["metadata_warnings"], serde_json::json!([]));
    assert_eq!(dry_run["data"]["verified_files"], 0);
    assert!(!dry_run_destination.exists());

    let latest_destination = temp.path().join("restore-latest");
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "restore",
            "--latest",
            latest_destination
                .to_str()
                .expect("latest destination path"),
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains("Restored snapshot"))
        .stderr("");
    assert_eq!(
        fs::read(latest_destination.join("sample.txt")).expect("latest restored file"),
        b"sample"
    );
}

#[cfg(unix)]
#[test]
fn restore_writes_directory_entries_and_symlinks_from_committed_snapshot() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::create_dir_all(source.join("empty/nested")).expect("create empty tree");
    fs::write(source.join("target.txt"), b"target").expect("write target");
    fs::set_permissions(source.join("target.txt"), fs::Permissions::from_mode(0o640))
        .expect("set source file mode");
    symlink("target.txt", source.join("target.link")).expect("create symlink");
    let backup = backup_source(&repo_url, passphrase, &source);

    let destination = temp.path().join("restore");
    let restore_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "restore",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let restore: Value = serde_json::from_slice(&restore_output).expect("restore json");
    assert_eq!(
        restore["data"]["snapshot_id"],
        backup["data"]["snapshot_id"]
    );
    assert_eq!(restore["data"]["entries_selected"], 5);
    assert_eq!(restore["data"]["directories_written"], 3);
    assert_eq!(restore["data"]["files_written"], 1);
    assert_eq!(restore["data"]["symlinks_written"], 1);
    assert_eq!(
        restore["data"]["metadata_planned"],
        expected_restore_metadata_fields(4)
    );
    assert_eq!(
        restore["data"]["metadata_applied"],
        expected_restore_metadata_fields(4)
    );
    assert_eq!(restore["data"]["metadata_warnings"], serde_json::json!([]));
    assert!(destination.join("empty/nested").is_dir());
    assert_eq!(
        fs::read(destination.join("target.txt")).expect("restored target"),
        b"target"
    );
    assert_eq!(
        fs::read_link(destination.join("target.link")).expect("restored symlink"),
        std::path::PathBuf::from("target.txt")
    );
    assert_eq!(
        fs::metadata(destination.join("target.txt"))
            .expect("restored target metadata")
            .permissions()
            .mode()
            & 0o777,
        0o640
    );

    let blocked_destination = temp.path().join("blocked");
    fs::create_dir(&blocked_destination).expect("create blocked destination");
    symlink(temp.path(), blocked_destination.join("target.link"))
        .expect("create destination symlink");
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "restore",
            "--overwrite",
            "--path",
            "target.link",
            blocked_destination
                .to_str()
                .expect("blocked destination path"),
        ])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicates::str::contains("contains a symlink"));
}

#[test]
#[cfg(unix)]
fn restore_path_scoped_symlink_creates_missing_parent_directory() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir_all(source.join("links")).expect("create source links dir");
    fs::write(source.join("target.txt"), b"target").expect("write target");
    symlink("../target.txt", source.join("links/target.link")).expect("create symlink");
    let backup = backup_source(&repo_url, passphrase, &source);

    let destination = temp.path().join("restore");
    let restore_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "restore",
            "--path",
            "links/target.link",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let restore: Value = serde_json::from_slice(&restore_output).expect("restore json");

    assert_eq!(
        restore["data"]["snapshot_id"],
        backup["data"]["snapshot_id"]
    );
    assert_eq!(restore["data"]["entries_selected"], 1);
    assert_eq!(restore["data"]["directories_written"], 0);
    assert_eq!(restore["data"]["files_written"], 0);
    assert_eq!(restore["data"]["symlinks_written"], 1);
    assert!(destination.join("links").is_dir());
    assert_eq!(
        fs::read_link(destination.join("links/target.link")).expect("restored symlink"),
        std::path::PathBuf::from("../target.txt")
    );
}

#[test]
fn restore_jsonl_emits_progress_events_without_stderr() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let destination = temp.path().join("restore");
    let restore_jsonl_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "restore",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = restore_jsonl_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 8);
    let started: Value = serde_json::from_slice(lines[0]).expect("started event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "restore");
    let progress: Vec<Value> = lines[1..7]
        .iter()
        .map(|line| serde_json::from_slice(line).expect("progress event"))
        .collect();
    assert_eq!(progress[0]["event"], "progress");
    assert_eq!(progress[0]["data"]["phase"], "load_manifest");
    assert_eq!(progress[5]["data"]["phase"], "complete");
    let completed: Value = serde_json::from_slice(lines[7]).expect("completed event");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["data"]["files_written"], 1);
    assert_eq!(
        completed["data"]["metadata_planned"],
        expected_restore_metadata_fields(2)
    );
    assert_eq!(
        completed["data"]["metadata_applied"],
        expected_restore_metadata_fields(2)
    );
}

#[test]
fn restore_requires_correct_password_and_safe_destination() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let destination = temp.path().join("restore");
    fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-passphrase")
        .args([
            "--repo",
            &repo_url,
            "restore",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .code(4)
        .stdout("")
        .stderr(predicates::str::contains(
            "repository could not be unlocked",
        ));

    fs::create_dir(&destination).expect("create destination");
    fs::write(destination.join("sample.txt"), b"existing").expect("write existing file");
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "restore",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicates::str::contains("already exists"));
    assert_eq!(
        fs::read(destination.join("sample.txt")).expect("existing file remains"),
        b"existing"
    );

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "restore",
            "--overwrite",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .success()
        .stderr("");
    assert_eq!(
        fs::read(destination.join("sample.txt")).expect("overwritten file"),
        b"sample"
    );
}

#[test]
fn restore_json_failure_preflights_destination_conflicts_before_writes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::create_dir(source.join("early")).expect("create early directory");
    fs::write(source.join("conflict.txt"), b"new").expect("write source conflict");
    backup_source(&repo_url, passphrase, &source);

    let destination = temp.path().join("restore");
    fs::create_dir(&destination).expect("create destination");
    fs::write(destination.join("conflict.txt"), b"old").expect("write destination conflict");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "restore",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failure: Value = serde_json::from_slice(&output).expect("restore failure json");

    assert_eq!(failure["command"], "restore");
    assert_eq!(failure["status"], "failure");
    assert_eq!(failure["data"]["code"], "restore_destination_exists");
    assert_eq!(failure["data"]["exit_code"], 2);
    assert!(
        failure["data"]["path"]
            .as_str()
            .expect("failure path")
            .ends_with("conflict.txt")
    );
    assert!(!destination.join("early").exists());
    assert_eq!(
        fs::read(destination.join("conflict.txt")).expect("existing destination file"),
        b"old"
    );
}

#[test]
fn restore_json_failure_reports_missing_requested_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let destination = temp.path().join("restore");
    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "restore",
            "--path",
            "missing.txt",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failure: Value = serde_json::from_slice(&output).expect("restore failure json");

    assert_eq!(failure["command"], "restore");
    assert_eq!(failure["status"], "failure");
    assert_eq!(failure["data"]["code"], "snapshot_path_not_found");
    assert_eq!(failure["data"]["exit_code"], 7);
    assert_eq!(failure["data"]["path"], serde_json::json!("missing.txt"));
    assert!(!destination.exists());
}

#[test]
fn restore_jsonl_failure_reports_missing_referenced_chunk_without_destination_writes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);
    let chunk_path = find_first_file(repo.join("objects/chunk"));
    fs::remove_file(&chunk_path).expect("remove chunk");

    let destination = temp.path().join("restore");
    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "restore",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let started: Value = serde_json::from_slice(lines[0]).expect("started event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "restore");

    let failed: Value = serde_json::from_slice(lines[1]).expect("failed event");
    assert_eq!(failed["event"], "command_failed");
    assert_eq!(failed["command"], "restore");
    assert_eq!(failed["status"], "failure");
    assert_eq!(
        failed["data"]["code"],
        "repository_referenced_object_missing"
    );
    assert_eq!(failed["data"]["exit_code"], 6);
    assert!(
        failed["data"]["object_key"]
            .as_str()
            .expect("object key")
            .starts_with("objects/chunk/")
    );
    assert!(!destination.exists());
}

#[test]
fn check_verifies_initialized_local_repository() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let check_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "check"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let check: Value = serde_json::from_slice(&check_output).expect("check json");
    assert_eq!(check["command"], "check");
    assert_eq!(check["status"], "success");
    assert_eq!(check["data"]["metadata_objects_checked"], 3);
    assert_eq!(check["data"]["chunk_objects_checked"], 1);
    assert_eq!(check["data"]["read_data_mode"], "full");
    assert_eq!(check["data"]["read_data_subset"], serde_json::Value::Null);
    assert_eq!(check["data"]["errors"], serde_json::json!([]));
    assert_eq!(check["data"]["warnings"], serde_json::json!([]));
    assert!(check["data"]["bytes_read"].as_u64().expect("bytes read") > 0);

    let check_jsonl_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "check"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = check_jsonl_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 7);
    let progress: Vec<Value> = lines[1..6]
        .iter()
        .map(|line| serde_json::from_slice(line).expect("progress event"))
        .collect();
    assert_eq!(progress[0]["data"]["phase"], "load_commits");
    assert_eq!(progress[4]["data"]["phase"], "complete");
    let completed: Value = serde_json::from_slice(lines[6]).expect("completed event");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["data"]["read_data_mode"], "full");
}

#[test]
fn check_read_data_subset_reports_count_and_percent_modes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("a.bin"), patterned_bytes(1, 700_000)).expect("write a");
    fs::write(source.join("b.bin"), patterned_bytes(2, 800_000)).expect("write b");
    fs::write(source.join("c.bin"), patterned_bytes(3, 900_000)).expect("write c");
    backup_source(&repo_url, passphrase, &source);

    let count_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "check",
            "--read-data-subset",
            "1",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let count: Value = serde_json::from_slice(&count_output).expect("count subset json");
    assert_eq!(count["data"]["read_data_mode"], "subset");
    assert_eq!(count["data"]["read_data_subset"], "1");
    assert_eq!(count["data"]["chunk_objects_checked"], 1);

    let percent_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "check",
            "--read-data-subset",
            "50%",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = percent_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    let completed: Value = serde_json::from_slice(lines.last().expect("completed event"))
        .expect("percent completed jsonl event");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["data"]["read_data_mode"], "subset");
    assert_eq!(completed["data"]["read_data_subset"], "50%");
    assert!(
        completed["data"]["chunk_objects_checked"]
            .as_u64()
            .expect("checked chunks")
            >= 1
    );
}

#[test]
fn check_read_data_subset_rejects_invalid_arguments() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();

    for subset in ["0", "0%", "101%", "abc"] {
        fileferry()
            .args(["--repo", &repo_url, "check", "--read-data-subset", subset])
            .assert()
            .code(2);
    }
}

#[test]
fn check_read_data_subset_integrity_failure_exits_six() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("a.bin"), patterned_bytes(4, 700_000)).expect("write a");
    fs::write(source.join("b.bin"), patterned_bytes(5, 800_000)).expect("write b");
    backup_source(&repo_url, passphrase, &source);

    let chunk_path = find_first_file(repo.join("objects/chunk"));
    let mut bytes = fs::read(&chunk_path).expect("chunk bytes");
    bytes[0] ^= 0x01;
    fs::write(&chunk_path, bytes).expect("tamper selected chunk");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "check",
            "--read-data-subset",
            "1",
        ])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failure: Value = serde_json::from_slice(&output).expect("check failure json");
    assert_eq!(failure["command"], "check");
    assert_eq!(failure["status"], "failure");
    assert_eq!(failure["data"]["exit_code"], 6);
    assert!(
        failure["data"]["object_key"]
            .as_str()
            .expect("object key")
            .starts_with("objects/chunk/")
    );
}

#[test]
fn check_requires_initialized_repository_correct_password_and_authentic_chunks() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "check"])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicates::str::contains("repository is not initialized"));

    init_repo(&repo_url, passphrase);
    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-passphrase")
        .args(["--repo", &repo_url, "check"])
        .assert()
        .code(4)
        .stdout("")
        .stderr(predicates::str::contains(
            "repository could not be unlocked",
        ));

    let chunk_path = find_first_file(repo.join("objects/chunk"));
    let mut bytes = fs::read(&chunk_path).expect("chunk bytes");
    bytes[0] ^= 0x01;
    fs::write(&chunk_path, bytes).expect("tamper chunk");
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "check"])
        .assert()
        .code(6)
        .stdout("")
        .stderr(predicates::str::contains("framing could not be decoded"));
}

#[test]
fn check_json_failure_reports_missing_chunk_as_machine_readable_finding() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let chunk_path = find_first_file(repo.join("objects/chunk"));
    fs::remove_file(&chunk_path).expect("delete chunk");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "check"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failure: Value = serde_json::from_slice(&output).expect("check failure json");

    assert_eq!(failure["command"], "check");
    assert_eq!(failure["status"], "failure");
    assert_eq!(failure["data"]["code"], "repository_check_missing_object");
    assert_eq!(failure["data"]["exit_code"], 6);
    assert_eq!(failure["data"]["retryable"], false);
    assert!(
        failure["data"]["object_key"]
            .as_str()
            .expect("object key")
            .starts_with("objects/chunk/")
    );
    assert_eq!(
        failure["data"]["finding"]["code"],
        "repository_check_missing_object"
    );
    assert_eq!(failure["data"]["finding"]["severity"], "error");
    assert_eq!(
        failure["data"]["finding"]["object_key"],
        failure["data"]["object_key"]
    );
}

#[test]
fn check_jsonl_failure_reports_tampered_chunk_without_stderr() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let chunk_path = find_first_file(repo.join("objects/chunk"));
    let mut bytes = fs::read(&chunk_path).expect("chunk bytes");
    bytes[0] ^= 0x01;
    fs::write(&chunk_path, bytes).expect("tamper chunk");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "check"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let started: Value = serde_json::from_slice(lines[0]).expect("started event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "check");

    let failed: Value = serde_json::from_slice(lines[1]).expect("failed event");
    assert_eq!(failed["event"], "command_failed");
    assert_eq!(failed["command"], "check");
    assert_eq!(failed["status"], "failure");
    assert_eq!(failed["data"]["code"], "repository_object_decode_failed");
    assert_eq!(failed["data"]["exit_code"], 6);
    assert!(
        failed["data"]["object_key"]
            .as_str()
            .expect("object key")
            .starts_with("objects/chunk/")
    );
    assert_eq!(
        failed["data"]["finding"]["code"],
        "repository_object_decode_failed"
    );
}

#[test]
fn repository_open_failures_are_structured_and_redacted_in_machine_modes() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";

    let uninitialized_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "check"])
        .assert()
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let uninitialized: Value =
        serde_json::from_slice(&uninitialized_output).expect("uninitialized json");
    assert_eq!(uninitialized["command"], "check");
    assert_eq!(uninitialized["status"], "failure");
    assert_eq!(uninitialized["data"]["code"], "repository_not_initialized");
    assert_eq!(uninitialized["data"]["exit_code"], 3);

    let s3_url = "s3://test-bucket/team/repo";
    let missing_s3_env_output = fileferry()
        .args(["--repo", s3_url, "--json", "check"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing_s3_env_text =
        String::from_utf8(missing_s3_env_output.clone()).expect("missing s3 env utf8");
    let missing_s3_env: Value =
        serde_json::from_slice(&missing_s3_env_output).expect("missing s3 env json");
    assert_eq!(
        missing_s3_env["data"]["code"],
        "repository_s3_environment_missing"
    );
    assert_eq!(missing_s3_env["data"]["exit_code"], 2);
    assert!(missing_s3_env_text.contains("FILEFERRY_S3_ENDPOINT"));
    assert!(!missing_s3_env_text.contains("test-bucket"));
    assert!(!missing_s3_env_text.contains("team/repo"));

    init_repo(&repo_url, passphrase);
    let wrong_password_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-passphrase")
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_password_text =
        String::from_utf8(wrong_password_output.clone()).expect("wrong password utf8");
    let wrong_password: Value =
        serde_json::from_slice(&wrong_password_output).expect("wrong password json");
    assert_eq!(wrong_password["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong_password["data"]["exit_code"], 4);
    assert!(!wrong_password_text.contains("wrong-passphrase"));

    fs::write(repo.join("bootstrap"), b"not-json").expect("corrupt bootstrap");
    let bootstrap_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "snapshots"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = bootstrap_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let failed: Value = serde_json::from_slice(lines[1]).expect("bootstrap failed event");
    assert_eq!(failed["event"], "command_failed");
    assert_eq!(failed["data"]["code"], "repository_bootstrap_decode_failed");
    assert_eq!(failed["data"]["exit_code"], 6);
}

#[test]
fn s3_data_path_commands_require_s3_environment_before_password() {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let destination = temp.path().join("restore");
    let recovery = temp.path().join("recovery.ffrec");
    let key_slot_id = "a".repeat(64);
    fs::create_dir(&source).expect("source dir");

    let cases: Vec<(&str, Vec<String>)> = vec![
        (
            "backup",
            vec!["backup".to_owned(), source.display().to_string()],
        ),
        ("snapshots", vec!["snapshots".to_owned()]),
        ("ls", vec!["ls".to_owned()]),
        (
            "restore",
            vec!["restore".to_owned(), destination.display().to_string()],
        ),
        ("check", vec!["check".to_owned()]),
        (
            "forget",
            vec![
                "forget".to_owned(),
                "--keep-last".to_owned(),
                "1".to_owned(),
            ],
        ),
        ("prune", vec!["prune".to_owned(), "--dry-run".to_owned()]),
        ("key add", vec!["key".to_owned(), "add".to_owned()]),
        (
            "key remove",
            vec!["key".to_owned(), "remove".to_owned(), key_slot_id.clone()],
        ),
        (
            "key rotate",
            vec![
                "key".to_owned(),
                "rotate".to_owned(),
                "--retire-key-slot".to_owned(),
                key_slot_id,
            ],
        ),
        (
            "key export-recovery",
            vec![
                "key".to_owned(),
                "export-recovery".to_owned(),
                "--output".to_owned(),
                recovery.display().to_string(),
            ],
        ),
    ];

    for (command_name, command_args) in cases {
        let mut args = vec![
            "--repo".to_owned(),
            "s3://test-bucket/team/repo".to_owned(),
            "--json".to_owned(),
        ];
        args.extend(command_args);

        let output = fileferry()
            .args(args)
            .assert()
            .code(2)
            .stderr("")
            .get_output()
            .stdout
            .clone();
        let text = String::from_utf8(output.clone()).expect("missing env utf8");
        let failure: Value = serde_json::from_slice(&output).expect("missing env json");

        assert_eq!(failure["command"], command_name);
        assert_eq!(failure["status"], "failure");
        assert_eq!(failure["data"]["code"], "repository_s3_environment_missing");
        assert_eq!(failure["data"]["exit_code"], 2);
        assert!(text.contains("FILEFERRY_S3_ENDPOINT"));
        assert!(!text.contains("test-bucket"));
        assert!(!text.contains("team/repo"));
        assert!(!text.contains("FILEFERRY_PASSWORD"));
        assert!(!text.contains("FILEFERRY_S3_ACCESS_KEY_ID"));
    }
}

#[test]
fn repository_open_reports_unsupported_bootstrap_version_and_features_as_incompatible() {
    let temp = tempfile::tempdir().expect("tempdir");
    let passphrase = "test-passphrase";

    let version_repo = temp.path().join("version-repo");
    let version_repo_url = version_repo.display().to_string();
    init_repo(&version_repo_url, passphrase);
    let mut bootstrap: Value =
        serde_json::from_slice(&fs::read(version_repo.join("bootstrap")).expect("bootstrap bytes"))
            .expect("bootstrap json");
    bootstrap["format_version"] = serde_json::json!(999);
    fs::write(
        version_repo.join("bootstrap"),
        serde_json::to_vec(&bootstrap).expect("unsupported version json"),
    )
    .expect("write unsupported version");
    let version_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &version_repo_url, "--json", "snapshots"])
        .assert()
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let version_failure: Value =
        serde_json::from_slice(&version_output).expect("version failure json");
    assert_eq!(version_failure["command"], "snapshots");
    assert_eq!(version_failure["status"], "failure");
    assert_eq!(
        version_failure["data"]["code"],
        "repository_format_unsupported"
    );
    assert_eq!(version_failure["data"]["exit_code"], 3);

    let feature_repo = temp.path().join("feature-repo");
    let feature_repo_url = feature_repo.display().to_string();
    init_repo(&feature_repo_url, passphrase);
    let mut bootstrap: Value =
        serde_json::from_slice(&fs::read(feature_repo.join("bootstrap")).expect("bootstrap bytes"))
            .expect("bootstrap json");
    bootstrap["features"] = serde_json::json!(["future-feature"]);
    fs::write(
        feature_repo.join("bootstrap"),
        serde_json::to_vec(&bootstrap).expect("unsupported feature json"),
    )
    .expect("write unsupported feature");
    let feature_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &feature_repo_url, "--jsonl", "check"])
        .assert()
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = feature_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let failed: Value = serde_json::from_slice(lines[1]).expect("feature failed event");
    assert_eq!(failed["event"], "command_failed");
    assert_eq!(failed["data"]["code"], "repository_features_unsupported");
    assert_eq!(failed["data"]["exit_code"], 3);
}

#[test]
fn snapshots_json_failure_reports_missing_referenced_manifest_as_integrity_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let manifest_path = find_first_file(repo.join("objects/manifest"));
    fs::remove_file(&manifest_path).expect("delete manifest");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failure: Value = serde_json::from_slice(&output).expect("snapshots failure json");

    assert_eq!(failure["command"], "snapshots");
    assert_eq!(failure["status"], "failure");
    assert_eq!(
        failure["data"]["code"],
        "repository_referenced_object_missing"
    );
    assert_eq!(failure["data"]["exit_code"], 6);
    assert!(
        failure["data"]["object_key"]
            .as_str()
            .expect("object key")
            .starts_with("objects/manifest/")
    );
}

#[test]
fn check_machine_failures_report_malformed_commits_and_corrupted_metadata() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let manifest_path = find_first_file(repo.join("objects/manifest"));
    let mut manifest_frame: Value =
        serde_json::from_slice(&fs::read(&manifest_path).expect("manifest frame"))
            .expect("manifest frame json");
    let first_ciphertext_byte = manifest_frame["ciphertext"]
        .as_array_mut()
        .expect("ciphertext array")
        .first_mut()
        .expect("ciphertext byte");
    let byte = first_ciphertext_byte
        .as_u64()
        .expect("ciphertext byte value");
    *first_ciphertext_byte = serde_json::json!(byte ^ 0x01);
    fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest_frame).expect("tampered manifest json"),
    )
    .expect("tamper manifest");

    let metadata_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "check"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let metadata_failure: Value =
        serde_json::from_slice(&metadata_output).expect("metadata failure json");
    assert_eq!(
        metadata_failure["data"]["code"],
        "repository_object_authentication_failed"
    );
    assert!(
        metadata_failure["data"]["object_key"]
            .as_str()
            .expect("object key")
            .starts_with("objects/manifest/")
    );
    assert_eq!(
        metadata_failure["data"]["finding"]["object_key"],
        metadata_failure["data"]["object_key"]
    );

    init_repo(&repo_url, passphrase);
    let commit_path = find_first_file(repo.join("commits"));
    fs::write(&commit_path, b"not-json").expect("malform commit");
    let commit_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "check"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = commit_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let failed: Value = serde_json::from_slice(lines[1]).expect("commit failed event");
    assert_eq!(failed["event"], "command_failed");
    assert_eq!(failed["data"]["code"], "repository_commit_decode_failed");
    assert!(
        failed["data"]["object_key"]
            .as_str()
            .expect("object key")
            .starts_with("commits/")
    );
    assert_eq!(
        failed["data"]["finding"]["code"],
        "repository_commit_decode_failed"
    );
}

#[test]
fn local_repository_ignores_stale_temp_and_uncommitted_partial_objects() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let stale_temp = repo.join(".fileferry-tmp").join("interrupted.part");
    fs::create_dir_all(stale_temp.parent().expect("temp parent")).expect("create temp parent");
    fs::write(&stale_temp, b"partial temporary object").expect("write stale temp");

    let uncommitted_object = repo
        .join("objects")
        .join("manifest")
        .join("ff")
        .join("uncommitted-partial");
    fs::create_dir_all(uncommitted_object.parent().expect("object parent"))
        .expect("create object parent");
    fs::write(&uncommitted_object, b"not a committed manifest").expect("write uncommitted object");

    let snapshots_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(snapshots["data"]["snapshots"].as_array().unwrap().len(), 1);

    let check_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "check"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let check: Value = serde_json::from_slice(&check_output).expect("check json");
    assert_eq!(check["status"], "success");
    assert_eq!(check["data"]["metadata_objects_checked"], 3);
    assert_eq!(check["data"]["chunk_objects_checked"], 1);
    assert!(stale_temp.is_file());
}

#[cfg(unix)]
#[test]
fn backup_json_failure_reports_permission_denied_source_file() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    let locked = source.join("locked.txt");
    fs::write(&locked, b"locked").expect("write locked");
    let original_permissions = fs::metadata(&locked).expect("metadata").permissions();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("lock file");

    if fs::read(&locked).is_ok() {
        fs::set_permissions(&locked, original_permissions).expect("restore permissions");
        return;
    }

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "backup",
            source.to_str().expect("source path"),
        ])
        .assert()
        .code(5)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    fs::set_permissions(&locked, original_permissions).expect("restore permissions");
    let failure: Value = serde_json::from_slice(&output).expect("backup failure json");

    assert_eq!(failure["command"], "backup");
    assert_eq!(failure["status"], "failure");
    assert_eq!(failure["data"]["code"], "file_read_failed");
    assert_eq!(failure["data"]["exit_code"], 5);
    assert_eq!(failure["data"]["retryable"], true);
    assert!(
        failure["data"]["path"]
            .as_str()
            .expect("failure path")
            .ends_with("locked.txt")
    );
    assert_eq!(failure["data"]["object_key"], serde_json::Value::Null);
}

#[test]
fn init_s3_requires_environment_and_redacts_repository_url() {
    let output = fileferry()
        .env("FILEFERRY_PASSWORD", "test-passphrase")
        .args(["--repo", "s3://test-bucket/team/repo", "--json", "init"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output.clone()).expect("s3 init failure utf8");
    let failure: Value = serde_json::from_slice(&output).expect("s3 init failure json");

    assert_eq!(failure["command"], "init");
    assert_eq!(failure["status"], "failure");
    assert_eq!(failure["data"]["code"], "repository_s3_environment_missing");
    assert_eq!(failure["data"]["exit_code"], 2);
    assert!(text.contains("FILEFERRY_S3_ENDPOINT"));
    assert!(!text.contains("test-bucket"));
    assert!(!text.contains("team/repo"));
    assert!(!text.contains("test-passphrase"));

    let secret_output = fileferry()
        .env("FILEFERRY_PASSWORD", "test-passphrase")
        .args([
            "--repo",
            "s3://access:secret@example.com/bucket?token=sensitive",
            "--jsonl",
            "init",
        ])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let secret_text = String::from_utf8(secret_output.clone()).expect("secret failure utf8");
    let lines: Vec<_> = secret_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let failed: Value = serde_json::from_slice(lines[1]).expect("secret failure event");
    assert_eq!(failed["event"], "command_failed");
    assert_eq!(failed["data"]["code"], "repository_s3_url_invalid");
    assert!(secret_text.contains("s3://<redacted>"));
    assert!(!secret_text.contains("secret"));
    assert!(!secret_text.contains("sensitive"));
}

#[test]
fn init_s3_live_integration_when_env_is_enabled() {
    if std::env::var("FILEFERRY_S3_INIT_INTEGRATION")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }

    let bucket = required_env("FILEFERRY_S3_BUCKET");
    let endpoint = required_env("FILEFERRY_S3_ENDPOINT");
    let region = required_env("FILEFERRY_S3_REGION");
    let access_key_id = required_env("FILEFERRY_S3_ACCESS_KEY_ID");
    let secret_access_key = required_env("FILEFERRY_S3_SECRET_ACCESS_KEY");
    let test_prefix = required_env("FILEFERRY_S3_TEST_PREFIX");
    let repo_prefix = format!("{test_prefix}/cli-init-{}", unique_test_id());
    let repo_url = format!("s3://{bucket}/{repo_prefix}");
    let passphrase = "s3-init-test-passphrase";

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_S3_ENDPOINT", &endpoint)
        .env("FILEFERRY_S3_REGION", &region)
        .env("FILEFERRY_S3_ACCESS_KEY_ID", &access_key_id)
        .env("FILEFERRY_S3_SECRET_ACCESS_KEY", &secret_access_key)
        .env("FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE", "1")
        .args(["--repo", &repo_url, "--json", "init"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output.clone()).expect("s3 init output utf8");
    let init: Value = serde_json::from_slice(&output).expect("s3 init json");

    assert_eq!(init["command"], "init");
    assert_eq!(init["status"], "success");
    assert_eq!(init["data"]["backend"], "s3_compatible");
    assert_eq!(init["data"]["created"], true);
    assert_eq!(init["data"]["repository_url"], "s3://<redacted>");
    assert!(!text.contains(&bucket));
    assert!(!text.contains(&repo_prefix));
    assert!(!text.contains(&access_key_id));
    assert!(!text.contains(&secret_access_key));

    let cleanup_config = S3StoreConfig::new(
        bucket,
        region,
        endpoint,
        access_key_id,
        secret_access_key,
        ObjectKeyPrefix::new(repo_prefix).expect("test prefix"),
    )
    .expect("cleanup s3 config")
    .with_conditional_create(false);
    let cleanup_store = S3Store::new(cleanup_config).expect("cleanup s3 store");
    let runtime = tokio::runtime::Runtime::new().expect("cleanup runtime");
    runtime
        .block_on(cleanup_store.delete(&ObjectKey::new("bootstrap").expect("bootstrap key")))
        .expect("cleanup bootstrap");
}

#[test]
fn s3_data_path_live_integration_when_env_is_enabled() {
    if std::env::var("FILEFERRY_S3_DATA_INTEGRATION")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }

    let bucket = required_env("FILEFERRY_S3_BUCKET");
    let endpoint = required_env("FILEFERRY_S3_ENDPOINT");
    let region = required_env("FILEFERRY_S3_REGION");
    let access_key_id = required_env("FILEFERRY_S3_ACCESS_KEY_ID");
    let secret_access_key = required_env("FILEFERRY_S3_SECRET_ACCESS_KEY");
    let test_prefix = required_env("FILEFERRY_S3_TEST_PREFIX");
    let repo_prefix = format!("{test_prefix}/cli-data-{}", unique_test_id());
    let repo_url = format!("s3://{bucket}/{repo_prefix}");
    let passphrase = "s3-data-test-passphrase";
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let destination = temp.path().join("restore");
    fs::create_dir_all(source.join("nested")).expect("create source tree");
    fs::write(source.join("sample.txt"), b"s3 sample").expect("write sample");
    fs::write(source.join("nested").join("keep.txt"), b"s3 nested").expect("write nested");

    let sensitive_values = [
        bucket.as_str(),
        repo_prefix.as_str(),
        access_key_id.as_str(),
        secret_access_key.as_str(),
    ];
    let s3_context = S3LiveCommandContext {
        endpoint: &endpoint,
        region: &region,
        access_key_id: &access_key_id,
        secret_access_key: &secret_access_key,
        passphrase,
        sensitive_values: &sensitive_values,
    };

    run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "init"], 0);

    let backup_output = run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "backup",
            "--tag",
            "s3-data",
            source.to_str().expect("source path"),
        ],
        0,
    );
    let backup_text = String::from_utf8(backup_output.clone()).expect("backup utf8");
    let backup: Value = serde_json::from_slice(&backup_output).expect("backup json");
    let snapshot_id = backup["data"]["snapshot_id"].as_str().expect("snapshot id");
    assert_eq!(backup["command"], "backup");
    assert_eq!(backup["status"], "success");
    assert_eq!(backup["data"]["tags"], json!(["s3-data"]));
    assert_eq!(backup["data"]["files_backed_up"], 2);
    assert!(!backup_text.contains(&bucket));
    assert!(!backup_text.contains(&repo_prefix));
    assert!(!backup_text.contains(&access_key_id));
    assert!(!backup_text.contains(&secret_access_key));

    let snapshots_output =
        run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "snapshots"], 0);
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(
        snapshots["data"]["snapshots"][0]["snapshot_id"],
        snapshot_id
    );

    let ls_output = run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "ls",
            "--snapshot",
            snapshot_id,
        ],
        0,
    );
    let listing: Value = serde_json::from_slice(&ls_output).expect("ls json");
    let listed_paths = listing["data"]["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|entry| entry["path"].as_str().expect("entry path"))
        .collect::<Vec<_>>();
    assert!(listed_paths.contains(&"nested"));
    assert!(listed_paths.contains(&"sample.txt"));

    let restore_output = run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "restore",
            "--snapshot",
            snapshot_id,
            destination.to_str().expect("destination path"),
        ],
        0,
    );
    let restore: Value = serde_json::from_slice(&restore_output).expect("restore json");
    assert_eq!(restore["data"]["snapshot_id"], snapshot_id);
    assert_eq!(
        fs::read(destination.join("sample.txt")).expect("restored sample"),
        b"s3 sample"
    );
    assert_eq!(
        fs::read(destination.join("nested").join("keep.txt")).expect("restored nested"),
        b"s3 nested"
    );

    let check_output =
        run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "check"], 0);
    let check: Value = serde_json::from_slice(&check_output).expect("check json");
    assert_eq!(check["status"], "success");
    assert!(check["data"]["chunk_objects_checked"].as_u64().unwrap_or(0) > 0);

    let manifest_key = format!("objects/manifest/{}/{}", &snapshot_id[..2], snapshot_id);
    let cleanup_store = s3_cleanup_store(
        &bucket,
        &region,
        &endpoint,
        &access_key_id,
        &secret_access_key,
        &repo_prefix,
    );
    let runtime = tokio::runtime::Runtime::new().expect("s3 data cleanup runtime");
    runtime
        .block_on(cleanup_store.delete(&ObjectKey::new(&manifest_key).expect("manifest key")))
        .expect("delete manifest for missing-object check");

    let missing_manifest_output =
        run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "check"], 6);
    let missing_manifest: Value =
        serde_json::from_slice(&missing_manifest_output).expect("missing manifest json");
    assert_eq!(
        missing_manifest["data"]["code"],
        "repository_check_missing_object"
    );
    assert_eq!(missing_manifest["data"]["exit_code"], 6);
    assert_eq!(missing_manifest["data"]["object_key"], manifest_key);
    assert_eq!(
        missing_manifest["data"]["finding"]["code"],
        "repository_check_missing_object"
    );
    assert_eq!(
        missing_manifest["data"]["finding"]["object_key"],
        manifest_key
    );

    let keys = runtime
        .block_on(cleanup_store.list_prefix(&ObjectKeyPrefix::root()))
        .expect("list s3 data cleanup keys");
    for key in keys {
        runtime
            .block_on(cleanup_store.delete(&key))
            .expect("delete s3 data cleanup key");
    }
}

#[test]
fn s3_prune_live_integration_when_env_is_enabled() {
    if std::env::var("FILEFERRY_S3_PRUNE_INTEGRATION")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }

    let bucket = required_env("FILEFERRY_S3_BUCKET");
    let endpoint = required_env("FILEFERRY_S3_ENDPOINT");
    let region = required_env("FILEFERRY_S3_REGION");
    let access_key_id = required_env("FILEFERRY_S3_ACCESS_KEY_ID");
    let secret_access_key = required_env("FILEFERRY_S3_SECRET_ACCESS_KEY");
    let test_prefix = required_env("FILEFERRY_S3_TEST_PREFIX");
    let repo_prefix = format!("{test_prefix}/cli-prune-{}", unique_test_id());
    let repo_url = format!("s3://{bucket}/{repo_prefix}");
    let passphrase = "s3-prune-test-passphrase";
    let temp = tempfile::tempdir().expect("tempdir");
    let keep_source = temp.path().join("keep-source");
    let drop_source = temp.path().join("drop-source");
    fs::create_dir(&keep_source).expect("create keep source");
    fs::create_dir(&drop_source).expect("create drop source");
    fs::write(keep_source.join("keep.txt"), b"s3 keep prune").expect("write keep");
    fs::write(drop_source.join("drop.txt"), b"s3 drop prune").expect("write drop");

    let sensitive_values = [
        bucket.as_str(),
        repo_prefix.as_str(),
        access_key_id.as_str(),
        secret_access_key.as_str(),
    ];
    let s3_context = S3LiveCommandContext {
        endpoint: &endpoint,
        region: &region,
        access_key_id: &access_key_id,
        secret_access_key: &secret_access_key,
        passphrase,
        sensitive_values: &sensitive_values,
    };

    run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "init"], 0);
    run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "backup",
            "--tag",
            "keep",
            keep_source.to_str().expect("keep source path"),
        ],
        0,
    );
    run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "backup",
            "--tag",
            "drop",
            drop_source.to_str().expect("drop source path"),
        ],
        0,
    );
    run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--keep-tag",
            "keep",
        ],
        0,
    );

    let prune_dry_run_output = run_s3_json_command(
        &s3_context,
        ["--repo", &repo_url, "--json", "prune", "--dry-run"],
        0,
    );
    let prune_dry_run: Value =
        serde_json::from_slice(&prune_dry_run_output).expect("prune dry-run json");
    assert_eq!(prune_dry_run["data"]["dry_run"], true);
    assert_eq!(prune_dry_run["data"]["completed"], true);
    assert_eq!(prune_dry_run["data"]["recovery_state"], "dry_run");
    assert!(
        prune_dry_run["data"]["candidate_object_count"]
            .as_u64()
            .expect("candidate count")
            >= 4
    );
    assert_eq!(prune_dry_run["data"]["deleted_object_count"], 0);

    let prune_output =
        run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "prune"], 0);
    let prune: Value = serde_json::from_slice(&prune_output).expect("prune json");
    assert_eq!(prune["data"]["dry_run"], false);
    assert_eq!(prune["data"]["completed"], true);
    assert_eq!(prune["data"]["recovery_state"], "completed");
    assert_eq!(
        prune["data"]["deleted_object_count"],
        prune["data"]["candidate_object_count"]
    );
    assert!(
        prune["data"]["candidate_objects"]
            .as_array()
            .expect("candidate objects")
            .iter()
            .any(|object| object["kind"] == "commit")
    );

    let snapshots_output =
        run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "snapshots"], 0);
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(snapshots["data"]["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(snapshots["data"]["snapshots"][0]["tags"], json!(["keep"]));

    let cleanup_store = s3_cleanup_store(
        &bucket,
        &region,
        &endpoint,
        &access_key_id,
        &secret_access_key,
        &repo_prefix,
    );
    let runtime = tokio::runtime::Runtime::new().expect("s3 prune cleanup runtime");
    let keys = runtime
        .block_on(cleanup_store.list_prefix(&ObjectKeyPrefix::root()))
        .expect("list s3 prune cleanup keys");
    assert!(
        keys.iter()
            .any(|key| key.as_str().starts_with("objects/prune-plan/"))
    );
    assert!(
        keys.iter()
            .any(|key| key.as_str().starts_with("objects/prune-completion/"))
    );
    for key in keys {
        runtime
            .block_on(cleanup_store.delete(&key))
            .expect("delete s3 prune cleanup key");
    }
}

#[test]
fn s3_retention_key_management_live_integration_when_env_is_enabled() {
    if std::env::var("FILEFERRY_S3_RETENTION_KEY_INTEGRATION")
        .ok()
        .as_deref()
        != Some("1")
    {
        return;
    }

    let bucket = required_env("FILEFERRY_S3_BUCKET");
    let endpoint = required_env("FILEFERRY_S3_ENDPOINT");
    let region = required_env("FILEFERRY_S3_REGION");
    let access_key_id = required_env("FILEFERRY_S3_ACCESS_KEY_ID");
    let secret_access_key = required_env("FILEFERRY_S3_SECRET_ACCESS_KEY");
    let test_prefix = required_env("FILEFERRY_S3_TEST_PREFIX");
    let repo_prefix = format!("{test_prefix}/cli-retention-key-{}", unique_test_id());
    let repo_url = format!("s3://{bucket}/{repo_prefix}");
    let passphrase = "s3-retention-key-passphrase";
    let added_passphrase = "s3-added-passphrase";
    let old_passphrase = "s3-old-passphrase";
    let rotated_passphrase = "s3-rotated-passphrase";
    let temp = tempfile::tempdir().expect("tempdir");
    let keep_source = temp.path().join("keep-source");
    let drop_source = temp.path().join("drop-source");
    let recovery = temp.path().join("recovery.ffrec");
    fs::create_dir(&keep_source).expect("create keep source");
    fs::create_dir(&drop_source).expect("create drop source");
    fs::write(keep_source.join("keep.txt"), b"s3 keep").expect("write keep");
    fs::write(drop_source.join("drop.txt"), b"s3 drop").expect("write drop");

    let sensitive_values = [
        bucket.as_str(),
        repo_prefix.as_str(),
        access_key_id.as_str(),
        secret_access_key.as_str(),
    ];
    let s3_context = S3LiveCommandContext {
        endpoint: &endpoint,
        region: &region,
        access_key_id: &access_key_id,
        secret_access_key: &secret_access_key,
        passphrase,
        sensitive_values: &sensitive_values,
    };

    run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "init"], 0);
    run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "backup",
            "--tag",
            "keep",
            keep_source.to_str().expect("keep source path"),
        ],
        0,
    );
    run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "backup",
            "--tag",
            "drop",
            drop_source.to_str().expect("drop source path"),
        ],
        0,
    );

    let forget_output = run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--keep-tag",
            "keep",
        ],
        0,
    );
    let forget: Value = serde_json::from_slice(&forget_output).expect("forget json");
    assert_eq!(forget["data"]["snapshots_forgotten"], 1);
    assert_eq!(forget["data"]["marker_objects_written"], 1);
    assert_eq!(forget["data"]["object_deletion"], false);
    assert!(
        forget["data"]["forgotten_snapshots"][0]["marker_object"]
            .as_str()
            .expect("forget marker")
            .starts_with("forgets/")
    );

    let snapshots_output =
        run_s3_json_command(&s3_context, ["--repo", &repo_url, "--json", "snapshots"], 0);
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(snapshots["data"]["snapshots"].as_array().unwrap().len(), 1);
    assert_eq!(snapshots["data"]["snapshots"][0]["tags"], json!(["keep"]));

    let key_add_output = s3_fileferry(&endpoint, &region, &access_key_id, &secret_access_key)
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .output()
        .expect("run key add");
    let key_add_output = assert_redacted_s3_output(key_add_output, 0, &sensitive_values);
    let key_add_text = String::from_utf8(key_add_output.clone()).expect("key add utf8");
    let key_add: Value = serde_json::from_slice(&key_add_output).expect("key add json");
    let added_key_slot_id = key_add["data"]["key_slot_id"]
        .as_str()
        .expect("added slot id")
        .to_owned();
    assert_eq!(key_add["data"]["key_slots"], 2);
    assert_eq!(key_add["data"]["reencrypted_repository_objects"], false);
    assert!(!key_add_text.contains(passphrase));
    assert!(!key_add_text.contains(added_passphrase));

    let added_context = S3LiveCommandContext {
        passphrase: added_passphrase,
        ..s3_context
    };
    run_s3_json_command(
        &added_context,
        ["--repo", &repo_url, "--json", "snapshots"],
        0,
    );

    let key_remove_output = run_s3_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "key",
            "remove",
            &added_key_slot_id,
        ],
        0,
    );
    let key_remove: Value = serde_json::from_slice(&key_remove_output).expect("key remove json");
    assert_eq!(key_remove["data"]["removed_key_slot_id"], added_key_slot_id);
    assert_eq!(key_remove["data"]["removal_marker_created"], true);
    assert_eq!(key_remove["data"]["deleted_key_slot_objects"], false);

    let removed_unlock_output = run_s3_json_command(
        &added_context,
        ["--repo", &repo_url, "--json", "snapshots"],
        4,
    );
    let removed_unlock: Value =
        serde_json::from_slice(&removed_unlock_output).expect("removed unlock json");
    assert_eq!(removed_unlock["data"]["code"], "repository_unlock_failed");

    let old_slot_output = s3_fileferry(&endpoint, &region, &access_key_id, &secret_access_key)
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", old_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .output()
        .expect("run old key add");
    let old_slot_output = assert_redacted_s3_output(old_slot_output, 0, &sensitive_values);
    let old_slot: Value = serde_json::from_slice(&old_slot_output).expect("old key add json");
    let old_key_slot_id = old_slot["data"]["key_slot_id"]
        .as_str()
        .expect("old slot id")
        .to_owned();

    let key_rotate_output = s3_fileferry(&endpoint, &region, &access_key_id, &secret_access_key)
        .env("FILEFERRY_PASSWORD", old_passphrase)
        .env("FILEFERRY_NEW_PASSWORD", rotated_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "rotate",
            "--retire-key-slot",
            &old_key_slot_id,
        ])
        .output()
        .expect("run key rotate");
    let key_rotate_output = assert_redacted_s3_output(key_rotate_output, 0, &sensitive_values);
    let key_rotate_text = String::from_utf8(key_rotate_output.clone()).expect("rotate utf8");
    let key_rotate: Value = serde_json::from_slice(&key_rotate_output).expect("rotate json");
    assert_eq!(
        key_rotate["data"]["removed_key_slot_ids"],
        json!([old_key_slot_id])
    );
    assert_eq!(key_rotate["data"]["removal_markers_created"], 1);
    assert_eq!(key_rotate["data"]["deleted_key_slot_objects"], false);
    assert_eq!(key_rotate["data"]["reencrypted_repository_objects"], false);
    assert!(!key_rotate_text.contains(old_passphrase));
    assert!(!key_rotate_text.contains(rotated_passphrase));

    let rotated_context = S3LiveCommandContext {
        passphrase: rotated_passphrase,
        ..s3_context
    };
    run_s3_json_command(
        &rotated_context,
        ["--repo", &repo_url, "--json", "snapshots"],
        0,
    );

    let old_context = S3LiveCommandContext {
        passphrase: old_passphrase,
        ..s3_context
    };
    let old_unlock_output = run_s3_json_command(
        &old_context,
        ["--repo", &repo_url, "--json", "snapshots"],
        4,
    );
    let old_unlock: Value = serde_json::from_slice(&old_unlock_output).expect("old unlock json");
    assert_eq!(old_unlock["data"]["code"], "repository_unlock_failed");

    let export_output = s3_fileferry(&endpoint, &region, &access_key_id, &secret_access_key)
        .env("FILEFERRY_PASSWORD", rotated_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "export-recovery",
            "--output",
            recovery.to_str().expect("recovery path"),
        ])
        .output()
        .expect("run export recovery");
    let export_output = assert_redacted_s3_output(export_output, 0, &sensitive_values);
    let export_text = String::from_utf8(export_output.clone()).expect("export utf8");
    let export: Value = serde_json::from_slice(&export_output).expect("export json");
    assert_eq!(export["data"]["raw_master_key_exported"], false);
    assert_eq!(export["data"]["reencrypted_repository_objects"], false);
    assert!(recovery.is_file());
    assert!(!export_text.contains(rotated_passphrase));

    let cleanup_store = s3_cleanup_store(
        &bucket,
        &region,
        &endpoint,
        &access_key_id,
        &secret_access_key,
        &repo_prefix,
    );
    let runtime = tokio::runtime::Runtime::new().expect("s3 retention cleanup runtime");
    let keys = runtime
        .block_on(cleanup_store.list_prefix(&ObjectKeyPrefix::root()))
        .expect("list s3 retention cleanup keys");
    for key in keys {
        runtime
            .block_on(cleanup_store.delete(&key))
            .expect("delete s3 retention cleanup key");
    }
}

fn required_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for S3 integration"))
}

fn unique_test_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

fn s3_fileferry(
    endpoint: &str,
    region: &str,
    access_key_id: &str,
    secret_access_key: &str,
) -> Command {
    let mut command = fileferry();
    command
        .env("FILEFERRY_S3_ENDPOINT", endpoint)
        .env("FILEFERRY_S3_REGION", region)
        .env("FILEFERRY_S3_ACCESS_KEY_ID", access_key_id)
        .env("FILEFERRY_S3_SECRET_ACCESS_KEY", secret_access_key)
        .env("FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE", "1");
    command
}

#[derive(Clone, Copy)]
struct S3LiveCommandContext<'a> {
    endpoint: &'a str,
    region: &'a str,
    access_key_id: &'a str,
    secret_access_key: &'a str,
    passphrase: &'a str,
    sensitive_values: &'a [&'a str],
}

fn run_s3_json_command<const N: usize>(
    context: &S3LiveCommandContext<'_>,
    args: [&str; N],
    expected_code: i32,
) -> Vec<u8> {
    let output = s3_fileferry(
        context.endpoint,
        context.region,
        context.access_key_id,
        context.secret_access_key,
    )
    .env("FILEFERRY_PASSWORD", context.passphrase)
    .args(args)
    .output()
    .expect("run ferry");
    assert_redacted_s3_output(output, expected_code, context.sensitive_values)
}

fn assert_redacted_s3_output(
    output: Output,
    expected_code: i32,
    sensitive_values: &[&str],
) -> Vec<u8> {
    let actual_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let redacted_stdout = redact_for_s3_test_failure(&stdout, sensitive_values);
    let redacted_stderr = redact_for_s3_test_failure(&stderr, sensitive_values);

    assert_eq!(
        actual_code, expected_code,
        "unexpected ferry exit code\nstdout={redacted_stdout}\nstderr={redacted_stderr}"
    );
    assert!(
        output.stderr.is_empty(),
        "expected empty stderr\nstdout={redacted_stdout}\nstderr={redacted_stderr}"
    );

    output.stdout
}

fn redact_for_s3_test_failure(text: &str, sensitive_values: &[&str]) -> String {
    let mut redacted = text.to_owned();
    for value in sensitive_values {
        if !value.is_empty() {
            redacted = redacted.replace(value, "<redacted>");
        }
    }
    redacted
}

fn s3_cleanup_store(
    bucket: &str,
    region: &str,
    endpoint: &str,
    access_key_id: &str,
    secret_access_key: &str,
    repo_prefix: &str,
) -> S3Store {
    let cleanup_config = S3StoreConfig::new(
        bucket.to_owned(),
        region.to_owned(),
        endpoint.to_owned(),
        access_key_id.to_owned(),
        secret_access_key.to_owned(),
        ObjectKeyPrefix::new(repo_prefix.to_owned()).expect("s3 cleanup prefix"),
    )
    .expect("s3 cleanup config")
    .with_conditional_create(false);
    S3Store::new(cleanup_config).expect("s3 cleanup store")
}

fn find_first_file(root: std::path::PathBuf) -> std::path::PathBuf {
    let mut pending = vec![root];
    while let Some(path) = pending.pop() {
        if path.is_file() {
            return path;
        }
        let mut children = fs::read_dir(&path)
            .expect("read dir")
            .map(|entry| entry.expect("dir entry").path())
            .collect::<Vec<_>>();
        children.sort();
        children.reverse();
        pending.extend(children);
    }
    panic!("file not found");
}

#[test]
fn backup_writes_committed_snapshot_that_snapshots_and_ls_can_discover() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "init"])
        .assert()
        .success()
        .stderr("");

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");

    let backup_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "backup",
            "--tag",
            "cli",
            source.to_str().expect("source path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let backup: Value = serde_json::from_slice(&backup_output).expect("backup json");
    assert_eq!(backup["command"], "backup");
    assert_eq!(backup["status"], "success");
    assert_eq!(backup["data"]["tags"], serde_json::json!(["cli"]));
    assert_eq!(backup["data"]["entries_scanned"], 2);
    assert_eq!(backup["data"]["files_backed_up"], 1);
    assert_eq!(backup["data"]["directories_backed_up"], 1);
    assert_eq!(backup["data"]["bytes_scanned"], 6);
    assert_eq!(backup["data"]["chunks_seen"], 1);
    assert_eq!(backup["data"]["chunks_written"], 1);
    assert_eq!(backup["data"]["chunks_reused"], 0);
    assert_eq!(backup["data"]["manifest_id"], backup["data"]["snapshot_id"]);
    assert_eq!(
        backup["data"]["index_ids"]
            .as_array()
            .expect("index id array")
            .len(),
        1
    );

    let snapshots_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    let snapshot = &snapshots["data"]["snapshots"][0];
    assert_eq!(snapshot["snapshot_id"], backup["data"]["snapshot_id"]);
    assert_eq!(snapshot["tags"], serde_json::json!(["cli"]));
    assert_eq!(snapshot["entry_count"], 2);

    let ls_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "ls"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let ls: Value = serde_json::from_slice(&ls_output).expect("ls json");
    assert_eq!(ls["command"], "ls");
    assert_eq!(ls["data"]["snapshot_id"], snapshot["snapshot_id"]);
    assert_eq!(ls["data"]["path"], ".");
    assert_eq!(ls["data"]["entries"][0]["path"], "sample.txt");
    assert_eq!(ls["data"]["entries"][0]["kind"], "regular_file");
    assert_eq!(ls["data"]["entries"][0]["size_bytes"], 6);
    assert_eq!(ls["data"]["entries"][0]["modified"]["status"], "captured");

    let snapshots_jsonl_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let jsonl_lines: Vec<_> = snapshots_jsonl_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(jsonl_lines.len(), 2);
    let completed: Value = serde_json::from_slice(jsonl_lines[1]).expect("completed event");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(
        completed["data"]["snapshots"][0]["snapshot_id"],
        snapshot["snapshot_id"]
    );

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "ls"])
        .assert()
        .success()
        .stdout(predicates::str::contains("file\t6\tsample.txt"))
        .stderr("");
}

#[test]
fn backup_jsonl_emits_progress_events_without_stderr() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "init"])
        .assert()
        .success()
        .stderr("");

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");

    let backup_jsonl_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "backup",
            "--tag",
            "cli",
            source.to_str().expect("source path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = backup_jsonl_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 9);
    let started: Value = serde_json::from_slice(lines[0]).expect("started event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "backup");
    let progress: Vec<Value> = lines[1..8]
        .iter()
        .map(|line| serde_json::from_slice(line).expect("progress event"))
        .collect();
    assert_eq!(progress[0]["event"], "progress");
    assert_eq!(progress[0]["data"]["phase"], "walk_sources");
    assert_eq!(progress[6]["data"]["phase"], "complete");
    let completed: Value = serde_json::from_slice(lines[8]).expect("completed event");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["data"]["tags"], serde_json::json!(["cli"]));
}

#[test]
fn backup_requires_initialized_repository_and_correct_password() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");

    fileferry()
        .env("FILEFERRY_PASSWORD", "test-passphrase")
        .args([
            "--repo",
            &repo_url,
            "backup",
            source.to_str().expect("source path"),
        ])
        .assert()
        .code(3)
        .stdout("")
        .stderr(predicates::str::contains("repository is not initialized"));

    fileferry()
        .env("FILEFERRY_PASSWORD", "test-passphrase")
        .args(["--repo", &repo_url, "init"])
        .assert()
        .success()
        .stderr("");

    fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-passphrase")
        .args([
            "--repo",
            &repo_url,
            "backup",
            source.to_str().expect("source path"),
        ])
        .assert()
        .code(4)
        .stdout("")
        .stderr(predicates::str::contains(
            "repository could not be unlocked",
        ));
}
