use assert_cmd::Command;
use fileferry_core::{
    BackupPipeline, BackupPipelineConfig, RepositoryLeaseCommandKind, RepositoryLeaseStateRequest,
    open_repository,
};
use fileferry_storage::{
    LocalStore, ObjectKey, ObjectKeyPrefix, ObjectStore, S3EndpointSecurity, S3Store, S3StoreConfig,
};
use predicates::prelude::PredicateBooleanExt;
use secrecy::SecretString;
use serde_json::{Value, json};
use std::{
    fs, io,
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
        "FILEFERRY_S3_ALLOW_INSECURE_HTTP",
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

fn raw_repository_bytes_contain(path: &Path, needle: &[u8]) -> bool {
    if !path.exists() {
        return false;
    }

    let mut pending = vec![path.to_path_buf()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).expect("read directory") {
            let entry = entry.expect("directory entry");
            let path = entry.path();
            let file_type = entry.file_type().expect("entry type");
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                let bytes = fs::read(&path).expect("read repository file");
                if bytes.windows(needle.len()).any(|window| window == needle) {
                    return true;
                }
            }
        }
    }

    false
}

fn expected_restore_metadata_planned_fields(
    entries_with_file_or_directory_metadata: usize,
) -> usize {
    if cfg!(unix) {
        entries_with_file_or_directory_metadata * 5
    } else {
        entries_with_file_or_directory_metadata * 2
    }
}

fn expected_restore_metadata_applied_fields(
    entries_with_file_or_directory_metadata: usize,
) -> usize {
    if cfg!(unix) {
        entries_with_file_or_directory_metadata * 4
    } else {
        entries_with_file_or_directory_metadata
    }
}

fn current_platform_json_name() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(unix) {
        "unix"
    } else {
        "unknown"
    }
}

fn platform_relative_path(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_owned()
    }
}

#[cfg(unix)]
fn expected_restore_symlink_metadata_fields(symlink_entries: usize) -> usize {
    symlink_entries * 5
}

fn set_modified_time(path: &Path, modified: SystemTime) {
    let file = fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open file for timestamp update");
    file.set_times(fs::FileTimes::new().set_modified(modified))
        .expect("set file modified time");
}

fn try_set_directory_modified_time(path: &Path, modified: SystemTime) -> bool {
    let directory = match fs::File::open(path) {
        Ok(directory) => directory,
        Err(source) if cfg!(windows) && source.kind() == io::ErrorKind::PermissionDenied => {
            return false;
        }
        Err(source) => panic!("open directory for timestamp update: {source}"),
    };

    match directory.set_times(fs::FileTimes::new().set_modified(modified)) {
        Ok(()) => true,
        Err(source) if cfg!(windows) && source.kind() == io::ErrorKind::PermissionDenied => false,
        Err(source) => panic!("set directory modified time: {source}"),
    }
}

#[cfg(unix)]
fn set_directory_modified_time(path: &Path, modified: SystemTime) {
    let directory = fs::File::open(path).expect("open directory for timestamp update");
    directory
        .set_times(fs::FileTimes::new().set_modified(modified))
        .expect("set directory modified time");
}

#[cfg(unix)]
fn test_xattr_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "com.fileferry.test"
    } else {
        "user.fileferry_test"
    }
}

fn patterned_bytes(seed: usize, len: usize) -> Vec<u8> {
    (0..len)
        .map(|index| ((index * 29 + seed * 11 + index / 3) % 251) as u8)
        .collect()
}

fn command_failure_json(output: &[u8]) -> Value {
    serde_json::from_slice(output).expect("failure json")
}

fn assert_doctor_corruption_is_redacted(
    repo_url: &str,
    passphrase: &str,
    expected_code: &str,
    redacted_object_prefix: &str,
) {
    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", repo_url, "--json", "doctor"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output.clone()).expect("doctor failure utf8");
    let failed = command_failure_json(&output);
    assert_eq!(failed["command"], "doctor");
    assert_eq!(failed["status"], "failure");
    assert_eq!(failed["data"]["code"], expected_code);
    assert_eq!(failed["data"]["exit_code"], 6);
    assert_eq!(failed["data"]["object_key"], Value::Null);
    assert_eq!(failed["data"]["path"], Value::Null);
    assert!(!text.contains(redacted_object_prefix));
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
fn repo_reports_safe_status_without_unlock_and_verifies_metadata_on_request() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "repo-status-passphrase";

    let uninitialized_output = fileferry()
        .args(["--repo", &repo_url, "--json", "repo"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let uninitialized: Value =
        serde_json::from_slice(&uninitialized_output).expect("uninitialized repo json");
    assert_eq!(uninitialized["command"], "repo");
    assert_eq!(uninitialized["status"], "success");
    assert_eq!(uninitialized["data"]["initialized"], false);
    assert!(uninitialized["data"]["repository_id"].is_null());
    assert_eq!(uninitialized["data"]["verification"], Value::Null);
    assert_eq!(
        uninitialized["data"]["storage"]["repository_requirements_met"],
        true
    );

    init_repo(&repo_url, passphrase);
    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("do-not-leak-source-name.txt"), b"contents").expect("write source");
    backup_source(&repo_url, passphrase, &source);

    let status_output = fileferry()
        .args(["--repo", &repo_url, "--json", "repo"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let status_text = String::from_utf8(status_output.clone()).expect("status utf8");
    let status: Value = serde_json::from_slice(&status_output).expect("repo status json");
    assert_eq!(status["data"]["initialized"], true);
    assert_eq!(status["data"]["format"]["compatibility"], "current");
    assert_eq!(status["data"]["verification"], Value::Null);
    assert!(!status_text.contains("do-not-leak-source-name"));

    let families = status["data"]["object_families"]
        .as_array()
        .expect("object family summaries");
    assert!(
        families
            .iter()
            .any(|family| family["family"] == "manifest" && family["objects"] == 1)
    );
    assert!(
        families
            .iter()
            .any(|family| family["family"] == "chunk" && family["objects"].as_u64().unwrap() > 0)
    );

    let verify_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "repo", "--verify"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines = verify_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice::<Value>(line).expect("repo jsonl event"))
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["event"], "command_started");
    assert_eq!(lines[1]["event"], "command_completed");
    assert_eq!(lines[1]["data"]["verification"]["unlocked"], true);
    assert_eq!(
        lines[1]["data"]["verification"]["read_data_mode"],
        "metadata_only"
    );
    assert_eq!(lines[1]["data"]["verification"]["chunk_objects_checked"], 0);
    assert!(
        lines[1]["data"]["verification"]["metadata_objects_checked"]
            .as_u64()
            .unwrap()
            >= 3
    );
}

#[test]
fn repo_inspects_unsupported_format_and_features_without_unlock() {
    let temp = tempfile::tempdir().expect("tempdir");
    let passphrase = "repo-migration-passphrase";

    let version_repo = temp.path().join("version-repo");
    let version_repo_url = version_repo.display().to_string();
    init_repo(&version_repo_url, passphrase);
    let mut bootstrap: Value =
        serde_json::from_slice(&fs::read(version_repo.join("bootstrap")).expect("bootstrap bytes"))
            .expect("bootstrap json");
    bootstrap["format_version"] = json!(999);
    fs::write(
        version_repo.join("bootstrap"),
        serde_json::to_vec(&bootstrap).expect("unsupported version json"),
    )
    .expect("write unsupported version");
    let version_output = fileferry()
        .args(["--repo", &version_repo_url, "--json", "repo"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let version: Value = serde_json::from_slice(&version_output).expect("version repo json");
    assert_eq!(version["data"]["initialized"], true);
    assert_eq!(
        version["data"]["format"]["compatibility"],
        "unsupported_future"
    );

    let feature_repo = temp.path().join("feature-repo");
    let feature_repo_url = feature_repo.display().to_string();
    init_repo(&feature_repo_url, passphrase);
    let mut bootstrap: Value =
        serde_json::from_slice(&fs::read(feature_repo.join("bootstrap")).expect("bootstrap bytes"))
            .expect("bootstrap json");
    bootstrap["features"] = json!(["future-feature"]);
    fs::write(
        feature_repo.join("bootstrap"),
        serde_json::to_vec(&bootstrap).expect("unsupported feature json"),
    )
    .expect("write unsupported feature");
    let feature_output = fileferry()
        .args(["--repo", &feature_repo_url, "--json", "repo"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let feature: Value = serde_json::from_slice(&feature_output).expect("feature repo json");
    assert_eq!(
        feature["data"]["format"]["compatibility"],
        "unsupported_features"
    );
    assert_eq!(
        feature["data"]["format"]["features"],
        json!(["future-feature"])
    );
}

#[test]
fn repo_verify_wrong_password_and_core_corruption_are_structured_and_redacted() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "repo-verify-passphrase";
    init_repo(&repo_url, passphrase);
    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), patterned_bytes(3, 700_000)).expect("write source");
    backup_source(&repo_url, passphrase, &source);

    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-repo-passphrase-canary")
        .args(["--repo", &repo_url, "--json", "repo", "--verify"])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_text = String::from_utf8(wrong_output.clone()).expect("wrong utf8");
    let wrong = command_failure_json(&wrong_output);
    assert_eq!(wrong["command"], "repo");
    assert_eq!(wrong["data"]["code"], "repository_unlock_failed");
    assert!(!wrong_text.contains("wrong-repo-passphrase-canary"));

    let bootstrap_repo = temp.path().join("bootstrap-corrupt");
    let bootstrap_repo_url = bootstrap_repo.display().to_string();
    init_repo(&bootstrap_repo_url, passphrase);
    fs::write(bootstrap_repo.join("bootstrap"), b"not-json").expect("corrupt bootstrap");
    let bootstrap_output = fileferry()
        .args(["--repo", &bootstrap_repo_url, "--json", "repo"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let bootstrap = command_failure_json(&bootstrap_output);
    assert_eq!(
        bootstrap["data"]["code"],
        "repository_bootstrap_decode_failed"
    );

    let manifest_repo = temp.path().join("manifest-corrupt");
    let manifest_repo_url = manifest_repo.display().to_string();
    init_repo(&manifest_repo_url, passphrase);
    backup_source(&manifest_repo_url, passphrase, &source);
    let manifest_path = find_first_file(manifest_repo.join("objects/manifest"));
    fs::write(&manifest_path, b"not encrypted manifest").expect("corrupt manifest");
    let manifest_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &manifest_repo_url, "--json", "repo", "--verify"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let manifest = command_failure_json(&manifest_output);
    assert_eq!(manifest["data"]["code"], "repository_object_decode_failed");
    assert!(
        manifest["data"]["object_key"]
            .as_str()
            .unwrap()
            .starts_with("objects/manifest/")
    );

    let index_repo = temp.path().join("index-corrupt");
    let index_repo_url = index_repo.display().to_string();
    init_repo(&index_repo_url, passphrase);
    backup_source(&index_repo_url, passphrase, &source);
    let index_path = find_first_file(index_repo.join("objects/index"));
    fs::write(&index_path, b"not encrypted index").expect("corrupt index");
    let index_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &index_repo_url, "--json", "repo", "--verify"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let index = command_failure_json(&index_output);
    assert_eq!(index["data"]["code"], "repository_object_decode_failed");
    assert!(
        index["data"]["object_key"]
            .as_str()
            .unwrap()
            .starts_with("objects/index/")
    );
}

#[test]
fn repo_verify_checks_lease_and_prune_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let passphrase = "repo-aux-passphrase";

    let lease_repo = temp.path().join("lease-repo");
    let lease_repo_url = lease_repo.display().to_string();
    init_repo(&lease_repo_url, passphrase);
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let store = LocalStore::new(&lease_repo);
    let opened = runtime
        .block_on(open_repository(
            &store,
            &SecretString::from(passphrase.to_owned()),
        ))
        .expect("open lease repo");
    let pipeline =
        BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id)).expect("pipeline");
    let lease_id = "ab".repeat(32);
    runtime
        .block_on(pipeline.write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: lease_id.clone(),
                writer_id: "cd".repeat(32),
                command_kind: RepositoryLeaseCommandKind::RepositoryMaintenance,
                acquired_at_unix_seconds: 10,
                expires_at_unix_seconds: 20,
            },
        ))
        .expect("write lease");
    fs::write(
        lease_repo.join("locks").join(&lease_id),
        b"not encrypted lease",
    )
    .expect("corrupt lease");
    let lease_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &lease_repo_url, "--json", "repo", "--verify"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lease = command_failure_json(&lease_output);
    assert_eq!(
        lease["data"]["code"],
        "repository_lease_state_decode_failed"
    );
    assert!(
        lease["data"]["object_key"]
            .as_str()
            .unwrap()
            .starts_with("locks/")
    );

    let prune_repo = temp.path().join("prune-repo");
    let prune_repo_url = prune_repo.display().to_string();
    init_repo(&prune_repo_url, passphrase);
    let source = temp.path().join("prune-source");
    fs::create_dir(&source).expect("create prune source");
    fs::write(source.join("sample.txt"), b"first").expect("write first");
    backup_source(&prune_repo_url, passphrase, &source);
    fs::write(source.join("sample.txt"), b"second").expect("write second");
    backup_source(&prune_repo_url, passphrase, &source);
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &prune_repo_url, "forget", "--keep-last", "1"])
        .assert()
        .success()
        .stderr("");
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &prune_repo_url, "prune"])
        .assert()
        .success()
        .stderr("");
    let prune_plan_path = find_first_file(prune_repo.join("objects/prune-plan"));
    fs::write(&prune_plan_path, b"not encrypted prune plan").expect("corrupt prune plan");
    let prune_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &prune_repo_url, "--json", "repo", "--verify"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let prune = command_failure_json(&prune_output);
    assert_eq!(prune["data"]["code"], "repository_prune_plan_decode_failed");
    assert!(
        prune["data"]["object_key"]
            .as_str()
            .unwrap()
            .starts_with("objects/prune-plan/")
    );
}

#[test]
fn doctor_reports_safe_health_without_backup_shape_by_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "doctor-passphrase";

    let uninitialized_output = fileferry()
        .args(["--repo", &repo_url, "--json", "doctor"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let uninitialized: Value =
        serde_json::from_slice(&uninitialized_output).expect("doctor uninitialized json");
    assert_eq!(uninitialized["command"], "doctor");
    assert_eq!(uninitialized["status"], "success");
    assert_eq!(uninitialized["data"]["initialized"], false);
    assert_eq!(uninitialized["data"]["health"]["status"], "uninitialized");
    assert_eq!(uninitialized["data"]["health"]["checked"], false);
    assert!(uninitialized["data"]["verification"].is_null());
    assert!(uninitialized["data"]["object_families"].is_null());
    assert_eq!(uninitialized["data"]["repair"]["attempted"], false);
    assert_eq!(uninitialized["data"]["repair"]["available"], false);

    init_repo(&repo_url, passphrase);
    let source = temp.path().join("doctor-source");
    fs::create_dir(&source).expect("create source");
    fs::write(
        source.join("do-not-leak-doctor-source-name.txt"),
        b"doctor contents",
    )
    .expect("write source");
    backup_source_with_tags(&repo_url, passphrase, &source, &["secret-doctor-tag"]);

    let doctor_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "doctor"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let doctor_text = String::from_utf8(doctor_output.clone()).expect("doctor utf8");
    let doctor: Value = serde_json::from_slice(&doctor_output).expect("doctor json");
    assert_eq!(doctor["data"]["health"]["status"], "healthy");
    assert_eq!(doctor["data"]["health"]["checked"], true);
    assert_eq!(
        doctor["data"]["verification"]["read_data_mode"],
        "metadata_only"
    );
    assert_eq!(doctor["data"]["verification"]["chunk_objects_checked"], 0);
    assert!(doctor["data"]["object_families"].is_null());
    assert!(!doctor_text.contains("do-not-leak-doctor-source-name"));
    assert!(!doctor_text.contains("secret-doctor-tag"));
    assert!(!doctor_text.contains(passphrase));

    let counted_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "doctor",
            "--read-data-subset",
            "1",
            "--show-object-counts",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let events = String::from_utf8(counted_output)
        .expect("doctor jsonl utf8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("doctor jsonl event"))
        .collect::<Vec<_>>();
    assert_eq!(events[0]["event"], "command_started");
    assert_eq!(events.last().unwrap()["event"], "command_completed");
    assert!(
        events
            .iter()
            .any(|event| { event["event"] == "progress" && event["data"]["phase"] == "read_data" })
    );
    let completed = events.last().expect("completed event");
    assert_eq!(
        completed["data"]["verification"]["read_data_mode"],
        "subset"
    );
    assert!(
        completed["data"]["verification"]["chunk_objects_checked"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(
        completed["data"]["object_families"]
            .as_array()
            .expect("object family counts")
            .iter()
            .any(|family| family["family"] == "manifest" && family["objects"] == 1)
    );
}

#[test]
fn doctor_handles_incompatible_and_corrupt_bootstrap_without_unlock() {
    let temp = tempfile::tempdir().expect("tempdir");
    let passphrase = "doctor-bootstrap-passphrase";

    let version_repo = temp.path().join("version-repo");
    let version_repo_url = version_repo.display().to_string();
    init_repo(&version_repo_url, passphrase);
    let mut bootstrap: Value =
        serde_json::from_slice(&fs::read(version_repo.join("bootstrap")).expect("bootstrap bytes"))
            .expect("bootstrap json");
    bootstrap["format_version"] = json!(999);
    fs::write(
        version_repo.join("bootstrap"),
        serde_json::to_vec(&bootstrap).expect("unsupported version json"),
    )
    .expect("write unsupported version");

    let version_output = fileferry()
        .args(["--repo", &version_repo_url, "--json", "doctor"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let version: Value = serde_json::from_slice(&version_output).expect("doctor version json");
    assert_eq!(version["data"]["health"]["status"], "incompatible");
    assert_eq!(
        version["data"]["format"]["compatibility"],
        "unsupported_future"
    );
    assert!(version["data"]["verification"].is_null());

    let corrupt_repo = temp.path().join("corrupt-repo");
    let corrupt_repo_url = corrupt_repo.display().to_string();
    init_repo(&corrupt_repo_url, passphrase);
    fs::write(corrupt_repo.join("bootstrap"), b"not-json").expect("corrupt bootstrap");
    let corrupt_output = fileferry()
        .args(["--repo", &corrupt_repo_url, "--json", "doctor"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let corrupt_text = String::from_utf8(corrupt_output.clone()).expect("corrupt utf8");
    let corrupt = command_failure_json(&corrupt_output);
    assert_eq!(
        corrupt["data"]["code"],
        "repository_bootstrap_decode_failed"
    );
    assert_eq!(corrupt["data"]["object_key"], Value::Null);
    assert!(!corrupt_text.contains("not-json"));
}

#[test]
fn doctor_failures_are_structured_redacted_and_hide_object_keys_by_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let passphrase = "doctor-corruption-passphrase";

    let wrong_repo = temp.path().join("wrong-repo");
    let wrong_repo_url = wrong_repo.display().to_string();
    init_repo(&wrong_repo_url, passphrase);
    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-doctor-passphrase-canary")
        .args(["--repo", &wrong_repo_url, "--json", "doctor"])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_text = String::from_utf8(wrong_output.clone()).expect("wrong utf8");
    let wrong = command_failure_json(&wrong_output);
    assert_eq!(wrong["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong["data"]["object_key"], Value::Null);
    assert_eq!(wrong["data"]["path"], Value::Null);
    assert!(!wrong_text.contains("wrong-doctor-passphrase-canary"));

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), patterned_bytes(5, 700_000)).expect("write source");

    let manifest_repo = temp.path().join("manifest-repo");
    let manifest_repo_url = manifest_repo.display().to_string();
    init_repo(&manifest_repo_url, passphrase);
    backup_source(&manifest_repo_url, passphrase, &source);
    fs::write(
        find_first_file(manifest_repo.join("objects/manifest")),
        b"not encrypted manifest",
    )
    .expect("corrupt manifest");
    assert_doctor_corruption_is_redacted(
        &manifest_repo_url,
        passphrase,
        "repository_object_decode_failed",
        "objects/manifest/",
    );

    let index_repo = temp.path().join("index-repo");
    let index_repo_url = index_repo.display().to_string();
    init_repo(&index_repo_url, passphrase);
    backup_source(&index_repo_url, passphrase, &source);
    fs::write(
        find_first_file(index_repo.join("objects/index")),
        b"not encrypted index",
    )
    .expect("corrupt index");
    assert_doctor_corruption_is_redacted(
        &index_repo_url,
        passphrase,
        "repository_object_decode_failed",
        "objects/index/",
    );

    let policy_repo = temp.path().join("policy-repo");
    let policy_repo_url = policy_repo.display().to_string();
    init_repo(&policy_repo_url, passphrase);
    let policy_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &policy_repo_url,
            "--json",
            "policy",
            "set",
            "--keep-daily",
            "3",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let policy: Value = serde_json::from_slice(&policy_output).expect("policy json");
    fs::write(
        policy_repo.join(
            policy["data"]["policy_object"]
                .as_str()
                .expect("policy object"),
        ),
        b"not encrypted policy",
    )
    .expect("corrupt policy");
    assert_doctor_corruption_is_redacted(
        &policy_repo_url,
        passphrase,
        "repository_policy_config_decode_failed",
        "objects/policy/",
    );

    let lease_repo = temp.path().join("lease-repo");
    let lease_repo_url = lease_repo.display().to_string();
    init_repo(&lease_repo_url, passphrase);
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let store = LocalStore::new(&lease_repo);
    let opened = runtime
        .block_on(open_repository(
            &store,
            &SecretString::from(passphrase.to_owned()),
        ))
        .expect("open lease repo");
    let pipeline =
        BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id)).expect("pipeline");
    let lease_id = "ab".repeat(32);
    runtime
        .block_on(pipeline.write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: lease_id.clone(),
                writer_id: "cd".repeat(32),
                command_kind: RepositoryLeaseCommandKind::RepositoryMaintenance,
                acquired_at_unix_seconds: 10,
                expires_at_unix_seconds: 20,
            },
        ))
        .expect("write lease");
    fs::write(
        lease_repo.join("locks").join(&lease_id),
        b"not encrypted lease",
    )
    .expect("corrupt lease");
    assert_doctor_corruption_is_redacted(
        &lease_repo_url,
        passphrase,
        "repository_lease_state_decode_failed",
        "locks/",
    );

    let prune_repo = temp.path().join("prune-repo");
    let prune_repo_url = prune_repo.display().to_string();
    init_repo(&prune_repo_url, passphrase);
    let prune_source = temp.path().join("prune-source");
    fs::create_dir(&prune_source).expect("create prune source");
    fs::write(prune_source.join("sample.txt"), b"first").expect("write first");
    backup_source(&prune_repo_url, passphrase, &prune_source);
    fs::write(prune_source.join("sample.txt"), b"second").expect("write second");
    backup_source(&prune_repo_url, passphrase, &prune_source);
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &prune_repo_url, "forget", "--keep-last", "1"])
        .assert()
        .success()
        .stderr("");
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &prune_repo_url, "prune"])
        .assert()
        .success()
        .stderr("");
    fs::write(
        find_first_file(prune_repo.join("objects/prune-plan")),
        b"not encrypted prune plan",
    )
    .expect("corrupt prune plan");
    assert_doctor_corruption_is_redacted(
        &prune_repo_url,
        passphrase,
        "repository_prune_plan_decode_failed",
        "objects/prune-plan/",
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
fn key_add_active_lease_has_stable_locked_exit_code_and_writes_no_slot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let added_passphrase = "added-passphrase";
    init_repo(&repo_url, passphrase);

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let store = LocalStore::new(&repo);
    let opened = runtime
        .block_on(open_repository(&store, &SecretString::from(passphrase)))
        .expect("open repository");
    let pipeline =
        BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id)).expect("pipeline");
    runtime
        .block_on(pipeline.write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: "a1".repeat(32),
                writer_id: "b2".repeat(32),
                command_kind: RepositoryLeaseCommandKind::KeyManagement,
                acquired_at_unix_seconds: 1,
                expires_at_unix_seconds: 4_000_000_000,
            },
        ))
        .expect("write active lease");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", added_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "add"])
        .assert()
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failed: Value = serde_json::from_slice(&output).expect("failure json");
    assert_eq!(failed["status"], "failure");
    assert_eq!(failed["data"]["code"], "repository_locked");
    assert_eq!(failed["data"]["exit_code"], 3);
    assert_eq!(file_count_under(&repo.join("key-slots")), 0);
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
fn key_remove_malformed_lease_has_stable_integrity_exit_code_and_writes_no_marker() {
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
    let lease_id = "c3".repeat(32);
    let lease_path = repo.join("locks").join(&lease_id);
    fs::create_dir_all(lease_path.parent().expect("lease parent")).expect("create locks dir");
    fs::write(&lease_path, b"not encrypted json").expect("write malformed lease");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "key", "remove", key_slot_id])
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
        "repository_lease_state_decode_failed"
    );
    assert_eq!(failed["data"]["exit_code"], 6);
    assert_eq!(failed["data"]["object_key"], format!("locks/{lease_id}"));
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 0);
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
fn key_rotate_active_lease_has_stable_locked_exit_code_and_writes_no_slot_or_marker() {
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

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let store = LocalStore::new(&repo);
    let opened = runtime
        .block_on(open_repository(&store, &SecretString::from(passphrase)))
        .expect("open repository");
    let pipeline =
        BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id)).expect("pipeline");
    runtime
        .block_on(pipeline.write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: "d4".repeat(32),
                writer_id: "e5".repeat(32),
                command_kind: RepositoryLeaseCommandKind::Prune,
                acquired_at_unix_seconds: 1,
                expires_at_unix_seconds: 4_000_000_000,
            },
        ))
        .expect("write active lease");

    let output = fileferry()
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
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failed: Value = serde_json::from_slice(&output).expect("failure json");
    assert_eq!(failed["status"], "failure");
    assert_eq!(failed["data"]["code"], "repository_locked");
    assert_eq!(failed["data"]["exit_code"], 3);
    assert_eq!(file_count_under(&repo.join("key-slots")), 1);
    assert_eq!(file_count_under(&repo.join("key-slot-removals")), 0);
}

#[test]
fn key_rekey_rewrites_repository_and_rejects_old_unlocks_without_leaking_secrets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "rekey-old-passphrase-canary";
    let added_passphrase = "rekey-added-passphrase-canary";
    let new_passphrase = "rekey-new-passphrase-canary";
    let secret_name = "rekey-secret-source-name-canary.txt";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join(secret_name), b"rekey payload").expect("write source");
    backup_source(&repo_url, passphrase, &source);

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
    assert_eq!(add["data"]["key_slots"], 2);

    let rekey_output = fileferry()
        .env("FILEFERRY_PASSWORD", added_passphrase)
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "rekey"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let rekey_text = String::from_utf8(rekey_output.clone()).expect("rekey utf8");
    let rekey: Value = serde_json::from_slice(&rekey_output).expect("key rekey json");
    assert_eq!(rekey["command"], "key rekey");
    assert_eq!(rekey["status"], "success");
    assert_eq!(rekey["data"]["old_key_slots"], 2);
    assert_eq!(rekey["data"]["new_key_slots"], 1);
    assert_eq!(rekey["data"]["snapshots_rewritten"], 1);
    assert_eq!(rekey["data"]["commits_rewritten"], 1);
    assert_eq!(rekey["data"]["manifests_rewritten"], 1);
    assert!(
        rekey["data"]["chunks_rewritten"]
            .as_u64()
            .expect("chunks rewritten")
            > 0
    );
    assert_eq!(rekey["data"]["old_key_slots_retired"], 1);
    assert_eq!(rekey["data"]["old_key_slot_objects_deleted"], 1);
    assert_eq!(rekey["data"]["old_unlocks_retained"], false);
    assert_eq!(rekey["data"]["raw_master_key_exported"], false);
    assert_eq!(rekey["data"]["reencrypted_repository_objects"], true);
    assert_eq!(rekey["data"]["recovery_state"], "started");
    assert_eq!(rekey["data"]["kdf"]["algorithm"], "argon2id_v19");
    assert!(!rekey_text.contains(passphrase));
    assert!(!rekey_text.contains(added_passphrase));
    assert!(!rekey_text.contains(new_passphrase));
    assert_eq!(file_count_under(&repo.join("key-slots")), 0);
    assert_eq!(file_count_under(&repo.join("objects/rekey")), 0);
    assert!(!raw_repository_bytes_contain(&repo, passphrase.as_bytes()));
    assert!(!raw_repository_bytes_contain(
        &repo,
        added_passphrase.as_bytes()
    ));
    assert!(!raw_repository_bytes_contain(
        &repo,
        new_passphrase.as_bytes()
    ));
    assert!(!raw_repository_bytes_contain(&repo, secret_name.as_bytes()));

    fileferry()
        .env("FILEFERRY_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "check"])
        .assert()
        .success()
        .stderr("");
    fileferry()
        .env("FILEFERRY_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("");

    for retired_passphrase in [passphrase, added_passphrase] {
        let old_unlock_output = fileferry()
            .env("FILEFERRY_PASSWORD", retired_passphrase)
            .args(["--repo", &repo_url, "--json", "snapshots"])
            .assert()
            .code(4)
            .stderr("")
            .get_output()
            .stdout
            .clone();
        let old_unlock: Value =
            serde_json::from_slice(&old_unlock_output).expect("old unlock json");
        assert_eq!(old_unlock["data"]["code"], "repository_unlock_failed");
    }

    let destination = temp.path().join("restore");
    let restore_output = fileferry()
        .env("FILEFERRY_PASSWORD", new_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "restore",
            "--path",
            secret_name,
            destination.to_str().expect("destination path"),
        ])
        .output()
        .expect("run restore");
    let restore_code = restore_output.status.code().unwrap_or(-1);
    assert!(
        restore_code == 0 || restore_code == 10,
        "unexpected restore exit code {restore_code}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&restore_output.stdout),
        String::from_utf8_lossy(&restore_output.stderr)
    );
    assert!(restore_output.stderr.is_empty());
    let restore: Value =
        serde_json::from_slice(&restore_output.stdout).expect("restore after rekey json");
    assert_eq!(restore["command"], "restore");
    assert_eq!(restore["status"], "success");
    assert_eq!(
        fs::read(destination.join(secret_name)).expect("restored file"),
        b"rekey payload"
    );
}

#[test]
fn key_rekey_supports_jsonl_and_new_password_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let new_passphrase = "new-passphrase";
    let new_password_file = temp.path().join("new-password.txt");
    fs::write(&new_password_file, format!("{new_passphrase}\n")).expect("write new password");
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");
    backup_source(&repo_url, passphrase, &source);

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "key",
            "rekey",
            "--new-password-file",
            new_password_file.to_str().expect("new password file path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let output_text = String::from_utf8(output.clone()).expect("jsonl utf8");
    assert!(!output_text.contains(passphrase));
    assert!(!output_text.contains(new_passphrase));
    let events = output_text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("jsonl event"))
        .collect::<Vec<_>>();
    assert_eq!(events.len(), 8);
    assert_eq!(events[0]["event"], "command_started");
    assert_eq!(events[0]["command"], "key rekey");
    let phases = events[1..7]
        .iter()
        .map(|event| event["data"]["phase"].as_str().expect("phase"))
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        vec![
            "load_bootstrap",
            "derive_new_master_key",
            "rewrite_objects",
            "switch_bootstrap",
            "cleanup_old_objects",
            "complete"
        ]
    );
    assert!(
        events[1..7]
            .iter()
            .all(|event| event["event"] == "progress" && event["command"] == "key rekey")
    );
    assert_eq!(events[7]["event"], "command_completed");
    assert_eq!(events[7]["command"], "key rekey");
    assert_eq!(events[7]["data"]["snapshots_rewritten"], 1);
    assert_eq!(events[7]["data"]["old_unlocks_retained"], false);
}

#[test]
fn key_rekey_malformed_state_has_stable_integrity_exit_code() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let new_passphrase = "new-passphrase";
    init_repo(&repo_url, passphrase);
    let state_id = "a".repeat(64);
    let state_path = repo.join("objects/rekey/aa").join(&state_id);
    fs::create_dir_all(state_path.parent().expect("state parent")).expect("create rekey dir");
    fs::write(&state_path, b"not-json").expect("write malformed rekey state");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "rekey"])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failed: Value = serde_json::from_slice(&output).expect("malformed rekey json");
    assert_eq!(failed["status"], "failure");
    assert_eq!(
        failed["data"]["code"],
        "repository_rekey_state_decode_failed"
    );
    assert_eq!(failed["data"]["exit_code"], 6);
    assert_eq!(
        failed["data"]["object_key"],
        format!("objects/rekey/aa/{state_id}")
    );
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn key_rekey_active_lease_has_stable_locked_exit_code_and_writes_no_state() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    let new_passphrase = "new-passphrase";
    init_repo(&repo_url, passphrase);

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let store = LocalStore::new(&repo);
    let opened = runtime
        .block_on(open_repository(&store, &SecretString::from(passphrase)))
        .expect("open repository");
    let pipeline =
        BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id)).expect("pipeline");
    runtime
        .block_on(pipeline.write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: "f6".repeat(32),
                writer_id: "a7".repeat(32),
                command_kind: RepositoryLeaseCommandKind::Backup,
                acquired_at_unix_seconds: 1,
                expires_at_unix_seconds: 4_000_000_000,
            },
        ))
        .expect("write active lease");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "rekey"])
        .assert()
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failed: Value = serde_json::from_slice(&output).expect("locked rekey json");
    assert_eq!(failed["status"], "failure");
    assert_eq!(failed["data"]["code"], "repository_locked");
    assert_eq!(failed["data"]["exit_code"], 3);
    assert_eq!(file_count_under(&repo.join("objects/rekey")), 0);
    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn key_rekey_preserves_stored_policy_for_explicit_forget() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "policy-rekey-passphrase-canary";
    let new_passphrase = "policy-rekey-new-passphrase-canary";
    let keep_tag = "policy-rekey-keep-tag-canary";
    init_repo(&repo_url, passphrase);

    let keep_source = temp.path().join("keep-source");
    let drop_source = temp.path().join("drop-source");
    fs::create_dir(&keep_source).expect("create keep source");
    fs::create_dir(&drop_source).expect("create drop source");
    fs::write(keep_source.join("keep.txt"), b"keep").expect("write keep");
    fs::write(drop_source.join("drop.txt"), b"drop").expect("write drop");
    backup_source_with_tags(&repo_url, passphrase, &keep_source, &[keep_tag]);
    backup_source_with_tags(&repo_url, passphrase, &drop_source, &["drop"]);

    let set_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "set",
            "--keep-tag",
            keep_tag,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let set: Value = serde_json::from_slice(&set_output).expect("policy set json");
    let old_policy_id = set["data"]["policy_id"].as_str().expect("old policy id");

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "key", "rekey"])
        .assert()
        .success()
        .stderr("");

    let show_output = fileferry()
        .env("FILEFERRY_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "policy", "show"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let show: Value = serde_json::from_slice(&show_output).expect("policy show json");
    assert_eq!(show["data"]["policy_count"], 1);
    let new_policy_id = show["data"]["policies"][0]["policy_id"]
        .as_str()
        .expect("new policy id");
    assert_ne!(new_policy_id, old_policy_id);
    assert_eq!(
        show["data"]["policies"][0]["retention"]["keep_tags"],
        json!([keep_tag])
    );

    let old_policy_output = fileferry()
        .env("FILEFERRY_PASSWORD", new_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--dry-run",
            "--policy",
            old_policy_id,
        ])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let old_policy: Value =
        serde_json::from_slice(&old_policy_output).expect("old policy failure json");
    assert_eq!(
        old_policy["data"]["code"],
        "repository_policy_config_not_found"
    );

    let forget_output = fileferry()
        .env("FILEFERRY_PASSWORD", new_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--policy",
            new_policy_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let forget: Value = serde_json::from_slice(&forget_output).expect("forget json");
    assert_eq!(forget["data"]["policy_source"], "stored");
    assert_eq!(forget["data"]["policy_id"], new_policy_id);
    assert_eq!(forget["data"]["snapshots_forgotten"], 1);

    let snapshots_output = fileferry()
        .env("FILEFERRY_PASSWORD", new_passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let snapshots: Value = serde_json::from_slice(&snapshots_output).expect("snapshots json");
    assert_eq!(
        snapshots["data"]["snapshots"]
            .as_array()
            .expect("snapshots")
            .len(),
        1
    );
    assert!(!raw_repository_bytes_contain(&repo, keep_tag.as_bytes()));
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
    assert_eq!(exported["data"]["recovery_import_implemented"], true);
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
fn key_import_recovery_adds_unlock_slot_without_leaking_secrets() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let export_path = temp.path().join("recovery.fileferry-key");
    let recovery_passphrase = "recovery-passphrase";
    let imported_passphrase = "imported-passphrase";
    init_repo(&repo_url, recovery_passphrase);
    let bootstrap_before = fs::read(repo.join("bootstrap")).expect("bootstrap before");
    fileferry()
        .env("FILEFERRY_PASSWORD", recovery_passphrase)
        .args([
            "--repo",
            &repo_url,
            "key",
            "export-recovery",
            "--output",
            export_path.to_str().expect("export path"),
        ])
        .assert()
        .success()
        .stderr("");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", recovery_passphrase)
        .env("FILEFERRY_NEW_PASSWORD", imported_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "import-recovery",
            "--input",
            export_path.to_str().expect("export path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let output_text = String::from_utf8(output.clone()).expect("import utf8");
    let imported: Value = serde_json::from_slice(&output).expect("import json");
    assert_eq!(imported["command"], "key import-recovery");
    assert_eq!(imported["status"], "success");
    assert_eq!(imported["data"]["key_slots"], 2);
    assert_eq!(imported["data"]["kdf"]["algorithm"], "argon2id_v19");
    assert_eq!(imported["data"]["aead"], "xchacha20_poly1305");
    assert_eq!(imported["data"]["raw_master_key_exported"], false);
    assert_eq!(imported["data"]["reencrypted_repository_objects"], false);
    assert!(imported["data"]["added_key_slot_id"].as_str().is_some());
    assert!(!output_text.contains(recovery_passphrase));
    assert!(!output_text.contains(imported_passphrase));
    assert!(!output_text.contains(&repo_url));
    assert_eq!(
        fs::read(repo.join("bootstrap")).expect("bootstrap after"),
        bootstrap_before
    );
    assert_eq!(file_count_under(&repo.join("key-slots")), 1);

    fileferry()
        .env("FILEFERRY_PASSWORD", imported_passphrase)
        .args(["--repo", &repo_url, "--json", "snapshots"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn key_import_recovery_supports_jsonl() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let export_path = temp.path().join("recovery.fileferry-key");
    let recovery_passphrase = "recovery-passphrase";
    let imported_passphrase = "imported-passphrase";
    init_repo(&repo_url, recovery_passphrase);
    fileferry()
        .env("FILEFERRY_PASSWORD", recovery_passphrase)
        .args([
            "--repo",
            &repo_url,
            "key",
            "export-recovery",
            "--output",
            export_path.to_str().expect("export path"),
        ])
        .assert()
        .success()
        .stderr("");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", recovery_passphrase)
        .env("FILEFERRY_NEW_PASSWORD", imported_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "key",
            "import-recovery",
            "--input",
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
    assert_eq!(started["command"], "key import-recovery");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["command"], "key import-recovery");
    assert_eq!(completed["data"]["raw_master_key_exported"], false);
}

#[test]
fn key_import_recovery_failures_are_structured_without_writing_key_slots() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let other_repo = temp.path().join("other-repo");
    let repo_url = repo.display().to_string();
    let other_repo_url = other_repo.display().to_string();
    let export_path = temp.path().join("recovery.fileferry-key");
    let tampered_path = temp.path().join("tampered.fileferry-key");
    let recovery_passphrase = "recovery-passphrase";
    let other_passphrase = recovery_passphrase;
    let imported_passphrase = "imported-passphrase";
    init_repo(&repo_url, recovery_passphrase);
    init_repo(&other_repo_url, other_passphrase);
    fileferry()
        .env("FILEFERRY_PASSWORD", recovery_passphrase)
        .args([
            "--repo",
            &repo_url,
            "key",
            "export-recovery",
            "--output",
            export_path.to_str().expect("export path"),
        ])
        .assert()
        .success()
        .stderr("");

    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-recovery-passphrase")
        .env("FILEFERRY_NEW_PASSWORD", imported_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "import-recovery",
            "--input",
            export_path.to_str().expect("export path"),
        ])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_text = String::from_utf8(wrong_output.clone()).expect("wrong import utf8");
    let wrong: Value = serde_json::from_slice(&wrong_output).expect("wrong import json");
    assert_eq!(wrong["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong["data"]["exit_code"], 4);
    assert!(!wrong_text.contains("wrong-recovery-passphrase"));
    assert!(!wrong_text.contains(imported_passphrase));
    assert_eq!(file_count_under(&repo.join("key-slots")), 0);

    let mismatch_output = fileferry()
        .env("FILEFERRY_PASSWORD", recovery_passphrase)
        .env("FILEFERRY_NEW_PASSWORD", imported_passphrase)
        .args([
            "--repo",
            &other_repo_url,
            "--json",
            "key",
            "import-recovery",
            "--input",
            export_path.to_str().expect("export path"),
        ])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let mismatch: Value = serde_json::from_slice(&mismatch_output).expect("mismatch import json");
    assert_eq!(
        mismatch["data"]["code"],
        "repository_recovery_export_invalid"
    );
    assert_eq!(mismatch["data"]["exit_code"], 6);
    assert_eq!(file_count_under(&other_repo.join("key-slots")), 0);

    let mut tampered: Value =
        serde_json::from_slice(&fs::read(&export_path).expect("read export")).expect("export json");
    tampered["recovery_key"]["wrapped_master_key"][0] = json!(0);
    fs::write(
        &tampered_path,
        serde_json::to_vec_pretty(&tampered).expect("tampered export json"),
    )
    .expect("write tampered export");
    let tampered_output = fileferry()
        .env("FILEFERRY_PASSWORD", recovery_passphrase)
        .env("FILEFERRY_NEW_PASSWORD", imported_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "import-recovery",
            "--input",
            tampered_path.to_str().expect("tampered path"),
        ])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let tampered_failure: Value =
        serde_json::from_slice(&tampered_output).expect("tampered import json");
    assert_eq!(tampered_failure["data"]["code"], "repository_unlock_failed");
    assert_eq!(file_count_under(&repo.join("key-slots")), 0);
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
    assert_eq!(file_count_under(&repo.join("locks")), 0);
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
fn forget_stored_policy_dry_run_jsonl_and_real_marker_write() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "stored-policy-passphrase-canary";
    let secret_tag = "stored-policy-secret-tag-canary";
    init_repo(&repo_url, passphrase);

    let keep_source = temp.path().join("keep-source");
    let drop_source = temp.path().join("drop-source");
    fs::create_dir(&keep_source).expect("create keep source");
    fs::create_dir(&drop_source).expect("create drop source");
    fs::write(keep_source.join("keep.txt"), b"keep").expect("write keep");
    fs::write(drop_source.join("drop.txt"), b"drop").expect("write drop");
    backup_source_with_tags(&repo_url, passphrase, &keep_source, &[secret_tag]);
    backup_source_with_tags(&repo_url, passphrase, &drop_source, &["drop"]);

    let set_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "set",
            "--keep-tag",
            secret_tag,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let set: Value = serde_json::from_slice(&set_output).expect("policy set json");
    let policy_id = set["data"]["policy_id"].as_str().expect("policy id");
    assert!(!raw_repository_bytes_contain(&repo, secret_tag.as_bytes()));

    let dry_run_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--dry-run",
            "--policy",
            policy_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let dry_run_text = String::from_utf8(dry_run_output.clone()).expect("dry-run json utf8");
    let dry_run: Value = serde_json::from_slice(&dry_run_output).expect("forget dry-run json");
    assert_eq!(dry_run["data"]["dry_run"], true);
    assert_eq!(dry_run["data"]["policy_source"], "stored");
    assert_eq!(dry_run["data"]["policy_id"], policy_id);
    assert_eq!(
        dry_run["data"]["policy_summary"]["keep_tags"],
        json!([secret_tag])
    );
    assert_eq!(dry_run["data"]["snapshots_forgotten"], 1);
    assert_eq!(dry_run["data"]["retained_snapshots"], 1);
    assert_eq!(dry_run["data"]["marker_objects_written"], 0);
    assert_eq!(
        dry_run["data"]["kept_snapshots"][0]["reasons"],
        json!([format!("keep-tag:{secret_tag}")])
    );
    assert!(!dry_run_text.contains(passphrase));
    assert_eq!(file_count_under(&repo.join("forgets")), 0);

    let jsonl_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "forget",
            "--dry-run",
            "--policy",
            policy_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let events = String::from_utf8(jsonl_output)
        .expect("jsonl utf8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("jsonl event"))
        .collect::<Vec<_>>();
    assert_eq!(events.first().unwrap()["event"], "command_started");
    assert_eq!(events.last().unwrap()["event"], "command_completed");
    assert_eq!(events.last().unwrap()["data"]["policy_source"], "stored");
    assert_eq!(events.last().unwrap()["data"]["policy_id"], policy_id);

    let real_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo", &repo_url, "--json", "forget", "--policy", policy_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let real: Value = serde_json::from_slice(&real_output).expect("real forget json");
    assert_eq!(real["data"]["dry_run"], false);
    assert_eq!(real["data"]["policy_source"], "stored");
    assert_eq!(real["data"]["policy_id"], policy_id);
    assert_eq!(real["data"]["marker_objects_written"], 1);
    assert!(
        real["data"]["forgotten_snapshots"][0]["marker_object"]
            .as_str()
            .expect("marker object")
            .starts_with("forgets/")
    );
    assert_eq!(file_count_under(&repo.join("forgets")), 1);
    assert!(!raw_repository_bytes_contain(&repo, secret_tag.as_bytes()));
}

#[test]
fn forget_stored_policy_failures_are_explicit_and_stable() {
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

    let keep_last_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "set",
            "--keep-last",
            "1",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let keep_last: Value = serde_json::from_slice(&keep_last_output).expect("policy set json");
    let keep_last_policy_id = keep_last["data"]["policy_id"].as_str().expect("policy id");

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "set",
            "--keep-tag",
            "first",
        ])
        .assert()
        .success()
        .stderr("");

    let implicit_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "forget", "--dry-run"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let implicit: Value =
        serde_json::from_slice(&implicit_output).expect("implicit selection failure json");
    assert_eq!(implicit["data"]["code"], "retention_policy_empty");
    assert_eq!(implicit["data"]["exit_code"], 2);

    let mixed_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--dry-run",
            "--policy",
            keep_last_policy_id,
            "--keep-last",
            "1",
        ])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let mixed: Value = serde_json::from_slice(&mixed_output).expect("mixed policy failure json");
    assert_eq!(mixed["data"]["code"], "forget_policy_selection_invalid");
    assert_eq!(mixed["data"]["exit_code"], 2);

    let missing_policy_id = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    let missing_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--dry-run",
            "--policy",
            missing_policy_id,
        ])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing: Value = serde_json::from_slice(&missing_output).expect("missing policy json");
    assert_eq!(
        missing["data"]["code"],
        "repository_policy_config_not_found"
    );
    assert_eq!(missing["data"]["exit_code"], 7);

    let wrong_password_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-passphrase")
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--dry-run",
            "--policy",
            keep_last_policy_id,
        ])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_password: Value =
        serde_json::from_slice(&wrong_password_output).expect("wrong password json");
    assert_eq!(wrong_password["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong_password["data"]["exit_code"], 4);

    fs::write(
        repo.join(
            keep_last["data"]["policy_object"]
                .as_str()
                .expect("policy object"),
        ),
        b"not encrypted policy",
    )
    .expect("corrupt policy");
    let tampered_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "forget",
            "--dry-run",
            "--policy",
            keep_last_policy_id,
        ])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let tampered: Value = serde_json::from_slice(&tampered_output).expect("tampered json");
    assert_eq!(
        tampered["data"]["code"],
        "repository_policy_config_decode_failed"
    );
    assert_eq!(tampered["data"]["exit_code"], 6);
    assert!(
        tampered["data"]["object_key"]
            .as_str()
            .expect("object key")
            .starts_with("objects/policy/")
    );
    assert_eq!(file_count_under(&repo.join("forgets")), 0);
}

#[test]
fn forget_active_lease_has_stable_locked_exit_code_and_writes_no_markers() {
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

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let store = LocalStore::new(&repo);
    let opened = runtime
        .block_on(open_repository(&store, &SecretString::from(passphrase)))
        .expect("open repository");
    let pipeline =
        BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id)).expect("pipeline");
    runtime
        .block_on(pipeline.write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: "13".repeat(32),
                writer_id: "24".repeat(32),
                command_kind: RepositoryLeaseCommandKind::Prune,
                acquired_at_unix_seconds: 1,
                expires_at_unix_seconds: 4_000_000_000,
            },
        ))
        .expect("write active lease");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "forget", "--keep-last", "1"])
        .assert()
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failed: Value = serde_json::from_slice(&output).expect("failure json");
    assert_eq!(failed["status"], "failure");
    assert_eq!(failed["data"]["code"], "repository_locked");
    assert_eq!(failed["data"]["exit_code"], 3);
    assert_eq!(file_count_under(&repo.join("forgets")), 0);

    let dry_run_output = fileferry()
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
    let dry_run: Value = serde_json::from_slice(&dry_run_output).expect("dry-run json");
    assert_eq!(dry_run["data"]["dry_run"], true);
    assert_eq!(dry_run["data"]["marker_objects_written"], 0);
}

#[test]
fn forget_malformed_lease_has_stable_integrity_exit_code_and_writes_no_markers() {
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

    let lease_id = "57".repeat(32);
    let lease_path = repo.join("locks").join(&lease_id);
    fs::create_dir_all(lease_path.parent().expect("lease parent")).expect("create locks dir");
    fs::write(&lease_path, b"not encrypted json").expect("write malformed lease");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "forget", "--keep-last", "1"])
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
        "repository_lease_state_decode_failed"
    );
    assert_eq!(failed["data"]["exit_code"], 6);
    assert_eq!(failed["data"]["object_key"], format!("locks/{lease_id}"));
    assert_eq!(file_count_under(&repo.join("forgets")), 0);
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
fn prune_active_lease_has_stable_locked_exit_code() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let store = LocalStore::new(&repo);
    let opened = runtime
        .block_on(open_repository(&store, &SecretString::from(passphrase)))
        .expect("open repository");
    let pipeline =
        BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id)).expect("pipeline");
    runtime
        .block_on(pipeline.write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: "ab".repeat(32),
                writer_id: "cd".repeat(32),
                command_kind: RepositoryLeaseCommandKind::Prune,
                acquired_at_unix_seconds: 1,
                expires_at_unix_seconds: 4_000_000_000,
            },
        ))
        .expect("write active lease");

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "prune"])
        .assert()
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failed: Value = serde_json::from_slice(&output).expect("failure json");
    assert_eq!(failed["status"], "failure");
    assert_eq!(failed["data"]["code"], "repository_locked");
    assert_eq!(failed["data"]["exit_code"], 3);
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
fn policy_set_show_and_delete_manage_encrypted_repository_policy_configs() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "policy-passphrase-canary";
    let secret_tag = "policy-secret-tag-canary";
    init_repo(&repo_url, passphrase);

    let set_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "set",
            "--keep-last",
            "7",
            "--keep-daily",
            "14",
            "--keep-tag",
            secret_tag,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let set_text = String::from_utf8(set_output.clone()).expect("set json utf8");
    let set: Value = serde_json::from_slice(&set_output).expect("policy set json");
    assert_eq!(set["command"], "policy set");
    assert_eq!(set["status"], "success");
    assert_eq!(set["data"]["created"], true);
    assert_eq!(set["data"]["encrypted_at_rest"], true);
    assert_eq!(set["data"]["applied_to_forget"], false);
    assert_eq!(set["data"]["retention"]["keep_last"], 7);
    assert_eq!(set["data"]["retention"]["keep_daily"], 14);
    assert_eq!(set["data"]["retention"]["keep_tags"], json!([secret_tag]));
    assert!(!set_text.contains(passphrase));
    let policy_id = set["data"]["policy_id"].as_str().expect("policy id");
    let policy_object = set["data"]["policy_object"]
        .as_str()
        .expect("policy object");
    assert_eq!(policy_id.len(), 64);
    assert!(repo.join(policy_object).is_file());
    assert!(!raw_repository_bytes_contain(&repo, secret_tag.as_bytes()));

    let policy_files_before_idempotent_set = file_count_under(&repo.join("objects/policy"));
    let idempotent_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "set",
            "--keep-last",
            "7",
            "--keep-daily",
            "14",
            "--keep-tag",
            secret_tag,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let idempotent: Value =
        serde_json::from_slice(&idempotent_output).expect("idempotent policy json");
    assert_eq!(idempotent["data"]["policy_id"], policy_id);
    assert_eq!(idempotent["data"]["created"], false);
    assert_eq!(
        file_count_under(&repo.join("objects/policy")),
        policy_files_before_idempotent_set
    );

    let show_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "policy", "show"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let show: Value = serde_json::from_slice(&show_output).expect("policy show json");
    assert_eq!(show["command"], "policy show");
    assert_eq!(show["data"]["policy_count"], 1);
    assert_eq!(show["data"]["policies"][0]["policy_id"], policy_id);
    assert_eq!(
        show["data"]["policies"][0]["retention"]["keep_tags"],
        json!([secret_tag])
    );

    let wrong_password_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-policy-passphrase")
        .args(["--repo", &repo_url, "--json", "policy", "show"])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_password: Value =
        serde_json::from_slice(&wrong_password_output).expect("wrong password json");
    assert_eq!(wrong_password["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong_password["data"]["exit_code"], 4);

    let dry_run_delete_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "delete",
            policy_id,
            "--dry-run",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let dry_run_delete: Value =
        serde_json::from_slice(&dry_run_delete_output).expect("dry-run delete json");
    assert_eq!(dry_run_delete["data"]["dry_run"], true);
    assert_eq!(dry_run_delete["data"]["deleted"], false);
    assert!(repo.join(policy_object).is_file());

    let delete_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "policy", "delete", policy_id])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let delete: Value = serde_json::from_slice(&delete_output).expect("delete json");
    assert_eq!(delete["command"], "policy delete");
    assert_eq!(delete["data"]["deleted"], true);
    assert!(!repo.join(policy_object).exists());

    let missing_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--json", "policy", "show"])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing: Value = serde_json::from_slice(&missing_output).expect("missing policy json");
    assert_eq!(
        missing["data"]["code"],
        "repository_policy_config_not_found"
    );
    assert_eq!(missing["data"]["exit_code"], 7);

    let invalid_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "set",
            "--keep-last",
            "0",
        ])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let invalid: Value = serde_json::from_slice(&invalid_output).expect("invalid policy json");
    assert_eq!(invalid["data"]["code"], "retention_policy_count_invalid");
    assert_eq!(invalid["data"]["exit_code"], 2);
}

#[test]
fn policy_jsonl_reports_ordered_started_and_completed_events() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "policy",
            "set",
            "--keep-weekly",
            "8",
        ])
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

    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["event"], "command_started");
    assert_eq!(events[0]["command"], "policy set");
    assert_eq!(events[1]["event"], "command_completed");
    assert_eq!(events[1]["command"], "policy set");
    assert_eq!(events[1]["data"]["retention"]["keep_weekly"], 8);
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
        .code(10)
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
        expected_restore_metadata_planned_fields(1)
    );
    assert_eq!(
        restore["data"]["metadata_applied"],
        expected_restore_metadata_applied_fields(1)
    );
    assert_eq!(
        restore["data"]["metadata_warnings"]
            .as_array()
            .expect("metadata warnings")
            .len(),
        1
    );
    assert_eq!(
        restore["data"]["metadata_warnings"][0]["entry_id"],
        serde_json::json!(platform_relative_path("nested/keep.txt"))
    );
    assert_eq!(
        restore["data"]["metadata_warnings"][0]["namespace"],
        serde_json::json!("portable")
    );
    assert_eq!(
        restore["data"]["metadata_warnings"][0]["field"],
        serde_json::json!("created")
    );
    assert_eq!(
        restore["data"]["metadata_warnings"][0]["destination_platform"],
        serde_json::json!(current_platform_json_name())
    );
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
        .code(10)
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
        expected_restore_metadata_planned_fields(4)
    );
    assert_eq!(dry_run["data"]["metadata_applied"], 0);
    assert_eq!(
        dry_run["data"]["metadata_warnings"]
            .as_array()
            .expect("dry-run metadata warnings")
            .len(),
        4
    );
    assert!(
        dry_run["data"]["metadata_warnings"]
            .as_array()
            .expect("dry-run metadata warnings")
            .iter()
            .all(|warning| warning["namespace"] == "portable" && warning["field"] == "created")
    );
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
        .code(10)
        .stdout(predicates::str::contains("Restored snapshot"))
        .stderr(predicates::str::contains(
            "created timestamp is not restored by this version",
        ));
    assert_eq!(
        fs::read(latest_destination.join("sample.txt")).expect("latest restored file"),
        b"sample"
    );
}

#[cfg(unix)]
#[test]
fn restore_json_reports_unrestored_xattr_warning_for_selected_file() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    let file = source.join("sample.txt");
    fs::write(&file, b"sample").expect("write sample");
    if xattr::set(&file, test_xattr_name(), b"present").is_err() {
        return;
    }
    backup_source(&repo_url, passphrase, &source);

    let destination = temp.path().join("restore");
    let restore_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "restore",
            "--path",
            "sample.txt",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .code(10)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let restore: Value = serde_json::from_slice(&restore_output).expect("restore json");

    assert_eq!(restore["data"]["entries_selected"], 1);
    assert_eq!(
        restore["data"]["metadata_planned"],
        expected_restore_metadata_planned_fields(1) + 1
    );
    assert!(
        restore["data"]["metadata_warnings"]
            .as_array()
            .expect("metadata warnings")
            .iter()
            .any(|warning| warning["entry_id"] == "sample.txt"
                && warning["namespace"] == current_platform_json_name()
                && warning["field"] == "xattrs"
                && warning["reason"]
                    .as_str()
                    .expect("warning reason")
                    .contains("not restored by this version"))
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
    let empty_modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_600_000_000);
    set_directory_modified_time(&source.join("empty"), empty_modified);
    fs::set_permissions(source.join("empty"), fs::Permissions::from_mode(0o750))
        .expect("set source directory mode");
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
        .code(10)
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
        expected_restore_metadata_planned_fields(4) + expected_restore_symlink_metadata_fields(1)
    );
    assert_eq!(
        restore["data"]["metadata_applied"],
        expected_restore_metadata_applied_fields(4)
    );
    assert_eq!(
        restore["data"]["metadata_warnings"]
            .as_array()
            .expect("metadata warnings")
            .len(),
        4 + expected_restore_symlink_metadata_fields(1)
    );
    assert!(
        restore["data"]["metadata_warnings"]
            .as_array()
            .expect("metadata warnings")
            .iter()
            .any(|warning| warning["entry_id"] == "target.txt"
                && warning["namespace"] == "portable"
                && warning["field"] == "created")
    );
    assert!(
        restore["data"]["metadata_warnings"]
            .as_array()
            .expect("metadata warnings")
            .iter()
            .any(|warning| warning["entry_id"] == "target.link"
                && warning["namespace"] == "unix"
                && warning["field"] == "mode")
    );
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
    assert_eq!(
        fs::metadata(destination.join("empty"))
            .expect("restored empty directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o750
    );
    assert_eq!(
        fs::metadata(destination.join("empty"))
            .expect("restored empty directory metadata")
            .modified()
            .expect("restored empty directory modified time"),
        empty_modified
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
fn restore_preserves_file_and_directory_modified_times_through_cli() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    let docs = source.join("docs");
    let report = docs.join("report.txt");
    fs::create_dir_all(&docs).expect("create source tree");
    fs::write(&report, b"mtime survives ferry restore").expect("write report");

    let file_modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_650_000_000);
    let directory_modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_550_000_000);
    set_modified_time(&report, file_modified);
    let directory_mtime_configured = try_set_directory_modified_time(&docs, directory_modified);

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
            "docs",
            destination.to_str().expect("destination path"),
        ])
        .assert()
        .code(10)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let restore: Value = serde_json::from_slice(&restore_output).expect("restore json");

    assert_eq!(
        restore["data"]["snapshot_id"],
        backup["data"]["snapshot_id"]
    );
    assert_eq!(restore["data"]["entries_selected"], 2);
    assert_eq!(restore["data"]["directories_written"], 1);
    assert_eq!(restore["data"]["files_written"], 1);
    assert_eq!(
        restore["data"]["metadata_planned"],
        expected_restore_metadata_planned_fields(2)
    );
    assert_eq!(
        fs::read(destination.join("docs/report.txt")).expect("restored report"),
        b"mtime survives ferry restore"
    );
    assert_eq!(
        fs::metadata(destination.join("docs/report.txt"))
            .expect("restored report metadata")
            .modified()
            .expect("restored report modified time"),
        file_modified
    );

    let warnings = restore["data"]["metadata_warnings"]
        .as_array()
        .expect("metadata warnings");
    assert!(warnings.iter().any(|warning| {
        warning["entry_id"] == platform_relative_path("docs/report.txt")
            && warning["namespace"] == "portable"
            && warning["field"] == "created"
            && warning["source_platform"] == current_platform_json_name()
            && warning["destination_platform"] == current_platform_json_name()
    }));

    if directory_mtime_configured {
        assert_eq!(
            fs::metadata(destination.join("docs"))
                .expect("restored docs metadata")
                .modified()
                .expect("restored docs modified time"),
            directory_modified
        );
    } else {
        assert!(warnings.iter().any(|warning| {
            warning["entry_id"] == platform_relative_path("docs")
                && warning["namespace"] == "portable"
                && warning["field"] == "modified"
                && warning["source_platform"] == current_platform_json_name()
                && warning["destination_platform"] == current_platform_json_name()
        }));
    }
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
        .code(10)
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
    assert_eq!(
        restore["data"]["metadata_planned"],
        expected_restore_symlink_metadata_fields(1)
    );
    assert_eq!(restore["data"]["metadata_applied"], 0);
    assert_eq!(
        restore["data"]["metadata_warnings"]
            .as_array()
            .expect("metadata warnings")
            .len(),
        expected_restore_symlink_metadata_fields(1)
    );
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
        .code(10)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let events: Vec<Value> = restore_jsonl_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("jsonl event"))
        .collect();
    let started = events.first().expect("started event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "restore");
    let progress: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "progress")
        .collect();
    assert_eq!(progress.len(), 6);
    assert_eq!(progress[0]["data"]["phase"], "load_manifest");
    assert_eq!(progress[5]["data"]["phase"], "complete");
    let warnings: Vec<_> = events
        .iter()
        .filter(|event| event["event"] == "warning")
        .collect();
    assert!(
        warnings
            .iter()
            .all(|warning| warning["command"] == "restore")
    );
    assert!(warnings.iter().any(|warning| {
        warning["data"]["namespace"] == "portable" && warning["data"]["field"] == "created"
    }));
    let completed = events.last().expect("completed event");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["data"]["files_written"], 1);
    assert_eq!(
        completed["data"]["metadata_planned"],
        expected_restore_metadata_planned_fields(2)
    );
    let expected_metadata_applied = if cfg!(windows) {
        expected_restore_metadata_applied_fields(1)
    } else {
        expected_restore_metadata_applied_fields(2)
    };
    assert_eq!(
        completed["data"]["metadata_applied"],
        expected_metadata_applied
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
        .code(10)
        .stderr(predicates::str::contains(
            "created timestamp is not restored by this version",
        ));
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
        ("key rekey", vec!["key".to_owned(), "rekey".to_owned()]),
        (
            "key export-recovery",
            vec![
                "key".to_owned(),
                "export-recovery".to_owned(),
                "--output".to_owned(),
                recovery.display().to_string(),
            ],
        ),
        (
            "key import-recovery",
            vec![
                "key".to_owned(),
                "import-recovery".to_owned(),
                "--input".to_owned(),
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

    let endpoint_output = fileferry()
        .env("FILEFERRY_PASSWORD", "test-passphrase")
        .env(
            "FILEFERRY_S3_ENDPOINT",
            "https://user:secret@s3.example.com?token=sensitive",
        )
        .env("FILEFERRY_S3_REGION", "us-west-001")
        .env("FILEFERRY_S3_ACCESS_KEY_ID", "visible-access-key")
        .env("FILEFERRY_S3_SECRET_ACCESS_KEY", "visible-secret-key")
        .args(["--repo", "s3://test-bucket/team/repo", "--json", "init"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let endpoint_text = String::from_utf8(endpoint_output.clone()).expect("endpoint failure utf8");
    let endpoint: Value = serde_json::from_slice(&endpoint_output).expect("endpoint failure json");
    assert_eq!(endpoint["data"]["code"], "repository_s3_config_invalid");
    assert!(endpoint_text.contains("endpoint must not contain credentials"));
    assert!(!endpoint_text.contains("user:secret"));
    assert!(!endpoint_text.contains("token=sensitive"));
    assert!(!endpoint_text.contains("visible-access-key"));
    assert!(!endpoint_text.contains("visible-secret-key"));
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

    let cleanup_config = S3StoreConfig::new_with_endpoint_security(
        bucket,
        region,
        endpoint,
        access_key_id,
        secret_access_key,
        ObjectKeyPrefix::new(repo_prefix).expect("test prefix"),
        s3_endpoint_security_from_env(),
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
        10,
    );
    let restore: Value = serde_json::from_slice(&restore_output).expect("restore json");
    assert_eq!(restore["data"]["snapshot_id"], snapshot_id);
    assert!(
        restore["data"]["metadata_warnings"]
            .as_array()
            .is_some_and(|warnings| !warnings.is_empty()),
        "restore should report the metadata warnings that drive partial-success exit code 10"
    );
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
fn s3_command_surface_live_integration_when_env_is_enabled() {
    if std::env::var("FILEFERRY_S3_COMMAND_SURFACE_INTEGRATION")
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
    let repo_prefix = format!("{test_prefix}/cli-command-surface-{}", unique_test_id());
    let repo_url = format!("s3://{bucket}/{repo_prefix}");
    let passphrase = "s3-command-surface-old-passphrase";
    let new_passphrase = "s3-command-surface-new-passphrase";
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("same.txt"), b"s3 same").expect("write same");
    fs::write(source.join("changed.txt"), b"s3 old").expect("write old changed");

    let sensitive_values = [
        bucket.as_str(),
        repo_prefix.as_str(),
        access_key_id.as_str(),
        secret_access_key.as_str(),
        passphrase,
        new_passphrase,
    ];
    let s3_context = S3LiveCommandContext {
        endpoint: &endpoint,
        region: &region,
        access_key_id: &access_key_id,
        secret_access_key: &secret_access_key,
        passphrase,
        sensitive_values: &sensitive_values,
    };

    run_s3_provider_json_command(&s3_context, ["--repo", &repo_url, "--json", "init"], 0);
    let first_backup_output = run_s3_provider_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "backup",
            "--tag",
            "alpha",
            source.to_str().expect("source path"),
        ],
        0,
    );
    let first_backup: Value =
        serde_json::from_slice(&first_backup_output).expect("first backup json");
    let first_snapshot_id = first_backup["data"]["snapshot_id"]
        .as_str()
        .expect("first snapshot id")
        .to_owned();

    fs::write(source.join("changed.txt"), b"s3 new").expect("write new changed");
    fs::write(source.join("added.txt"), b"s3 added").expect("write added");
    let second_backup_output = run_s3_provider_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "backup",
            "--tag",
            "beta",
            source.to_str().expect("source path"),
        ],
        0,
    );
    let second_backup: Value =
        serde_json::from_slice(&second_backup_output).expect("second backup json");
    let second_snapshot_id = second_backup["data"]["snapshot_id"]
        .as_str()
        .expect("second snapshot id")
        .to_owned();

    let find_output = run_s3_provider_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "find",
            "--all",
            "--name",
            "added.txt",
        ],
        0,
    );
    let find: Value = serde_json::from_slice(&find_output).expect("find json");
    assert_eq!(find["command"], "find");
    assert_eq!(find["data"]["snapshots_searched"], 2);
    assert_eq!(find["data"]["matches_count"], 1);
    assert_eq!(
        find["data"]["matches"][0]["snapshot_id"],
        second_snapshot_id
    );
    assert_eq!(find["data"]["matches"][0]["path"], "added.txt");

    let diff_output = run_s3_provider_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "diff",
            "--from-snapshot",
            &first_snapshot_id,
            "--to-snapshot",
            &second_snapshot_id,
        ],
        0,
    );
    let diff: Value = serde_json::from_slice(&diff_output).expect("diff json");
    assert_eq!(diff["command"], "diff");
    assert_eq!(diff["data"]["from_snapshot_id"], first_snapshot_id);
    assert_eq!(diff["data"]["to_snapshot_id"], second_snapshot_id);
    assert_eq!(diff["data"]["added_count"], 1);
    assert_eq!(diff["data"]["changed_count"], 1);
    assert!(
        diff["data"]["entries"]
            .as_array()
            .expect("diff entries")
            .iter()
            .any(|entry| entry["path"] == "same.txt" && entry["status"] == "unchanged")
    );

    let repo_output = run_s3_provider_json_command(
        &s3_context,
        ["--repo", &repo_url, "--json", "repo", "--verify"],
        0,
    );
    let repo: Value = serde_json::from_slice(&repo_output).expect("repo json");
    assert_eq!(repo["command"], "repo");
    assert_eq!(repo["data"]["backend"], "s3_compatible");
    assert_eq!(repo["data"]["initialized"], true);
    assert_eq!(repo["data"]["storage"]["repository_requirements_met"], true);
    assert_eq!(repo["data"]["verification"]["unlocked"], true);
    assert_eq!(repo["data"]["verification"]["chunk_objects_checked"], 0);

    let doctor_output =
        run_s3_provider_jsonl_command(&s3_context, ["--repo", &repo_url, "--jsonl", "doctor"], 0);
    let doctor_events = parse_jsonl_events(&doctor_output);
    assert_eq!(doctor_events[0]["event"], "command_started");
    let doctor_completed = doctor_events.last().expect("doctor completed event");
    assert_eq!(doctor_completed["command"], "doctor");
    assert_eq!(doctor_completed["event"], "command_completed");
    assert_eq!(doctor_completed["data"]["backend"], "s3_compatible");
    assert_eq!(doctor_completed["data"]["health"]["status"], "healthy");
    assert_eq!(
        doctor_completed["data"]["verification"]["chunk_objects_checked"],
        0
    );
    assert_eq!(doctor_completed["data"]["repair"]["attempted"], false);

    let policy_set_output = run_s3_provider_json_command(
        &s3_context,
        [
            "--repo",
            &repo_url,
            "--json",
            "policy",
            "set",
            "--keep-last",
            "1",
        ],
        0,
    );
    let policy_set: Value = serde_json::from_slice(&policy_set_output).expect("policy set json");
    let policy_id = policy_set["data"]["policy_id"]
        .as_str()
        .expect("policy id")
        .to_owned();
    assert_eq!(policy_set["command"], "policy set");
    assert_eq!(policy_set["data"]["encrypted_at_rest"], true);
    assert_eq!(policy_set["data"]["applied_to_forget"], false);

    let policy_show_output = run_s3_provider_json_command(
        &s3_context,
        ["--repo", &repo_url, "--json", "policy", "show"],
        0,
    );
    let policy_show: Value = serde_json::from_slice(&policy_show_output).expect("policy show json");
    assert_eq!(policy_show["command"], "policy show");
    assert_eq!(policy_show["data"]["policy_count"], 1);
    assert_eq!(policy_show["data"]["policies"][0]["policy_id"], policy_id);

    let rekey_output =
        s3_provider_fileferry(&endpoint, &region, &access_key_id, &secret_access_key)
            .env("FILEFERRY_PASSWORD", passphrase)
            .env("FILEFERRY_NEW_PASSWORD", new_passphrase)
            .args(["--repo", &repo_url, "--jsonl", "key", "rekey"])
            .output()
            .expect("run key rekey");
    let rekey_output = assert_redacted_s3_output(rekey_output, 0, &sensitive_values);
    let rekey_events = parse_jsonl_events(&rekey_output);
    assert_eq!(rekey_events[0]["command"], "key rekey");
    assert!(
        rekey_events.iter().any(
            |event| event["event"] == "progress" && event["data"]["phase"] == "rewrite_objects"
        )
    );
    let rekey_completed = rekey_events.last().expect("rekey completed event");
    assert_eq!(rekey_completed["event"], "command_completed");
    assert_eq!(rekey_completed["data"]["snapshots_rewritten"], 2);
    assert_eq!(rekey_completed["data"]["policies_rewritten"], 1);
    assert_eq!(rekey_completed["data"]["old_unlocks_retained"], false);
    assert_eq!(
        rekey_completed["data"]["reencrypted_repository_objects"],
        true
    );

    let old_unlock_output =
        run_s3_provider_json_command(&s3_context, ["--repo", &repo_url, "--json", "snapshots"], 4);
    let old_unlock: Value = serde_json::from_slice(&old_unlock_output).expect("old unlock json");
    assert_eq!(old_unlock["data"]["code"], "repository_unlock_failed");

    let new_context = S3LiveCommandContext {
        passphrase: new_passphrase,
        ..s3_context
    };
    let post_rekey_snapshots = run_s3_provider_json_command(
        &new_context,
        ["--repo", &repo_url, "--json", "snapshots"],
        0,
    );
    let post_rekey_snapshots: Value =
        serde_json::from_slice(&post_rekey_snapshots).expect("post rekey snapshots json");
    assert_eq!(
        post_rekey_snapshots["data"]["snapshots"]
            .as_array()
            .expect("snapshots")
            .len(),
        2
    );

    let post_rekey_policy = run_s3_provider_json_command(
        &new_context,
        ["--repo", &repo_url, "--json", "policy", "show"],
        0,
    );
    let post_rekey_policy: Value =
        serde_json::from_slice(&post_rekey_policy).expect("post rekey policy json");
    assert_eq!(post_rekey_policy["data"]["policy_count"], 1);
    assert_ne!(
        post_rekey_policy["data"]["policies"][0]["policy_id"],
        policy_id
    );
    assert_eq!(
        post_rekey_policy["data"]["policies"][0]["retention"]["keep_last"],
        1
    );

    let cleanup_store = s3_cleanup_store(
        &bucket,
        &region,
        &endpoint,
        &access_key_id,
        &secret_access_key,
        &repo_prefix,
    );
    let runtime = tokio::runtime::Runtime::new().expect("s3 command surface cleanup runtime");
    let keys = runtime
        .block_on(cleanup_store.list_prefix(&ObjectKeyPrefix::root()))
        .expect("list s3 command surface cleanup keys");
    for key in keys {
        runtime
            .block_on(cleanup_store.delete(&key))
            .expect("delete s3 command surface cleanup key");
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
    let imported_passphrase = "s3-imported-passphrase";
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

    let import_output = s3_fileferry(&endpoint, &region, &access_key_id, &secret_access_key)
        .env("FILEFERRY_PASSWORD", rotated_passphrase)
        .env("FILEFERRY_NEW_PASSWORD", imported_passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "key",
            "import-recovery",
            "--input",
            recovery.to_str().expect("recovery path"),
        ])
        .output()
        .expect("run import recovery");
    let import_output = assert_redacted_s3_output(import_output, 0, &sensitive_values);
    let import_text = String::from_utf8(import_output.clone()).expect("import utf8");
    let import: Value = serde_json::from_slice(&import_output).expect("import json");
    assert_eq!(import["command"], "key import-recovery");
    assert_eq!(import["data"]["raw_master_key_exported"], false);
    assert_eq!(import["data"]["reencrypted_repository_objects"], false);
    assert!(!import_text.contains(rotated_passphrase));
    assert!(!import_text.contains(imported_passphrase));

    let imported_context = S3LiveCommandContext {
        passphrase: imported_passphrase,
        ..s3_context
    };
    run_s3_json_command(
        &imported_context,
        ["--repo", &repo_url, "--json", "snapshots"],
        0,
    );

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

fn s3_endpoint_security_from_env() -> S3EndpointSecurity {
    if std::env::var("FILEFERRY_S3_ALLOW_INSECURE_HTTP")
        .ok()
        .is_some_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
    {
        S3EndpointSecurity::AllowInsecureLocalHttp
    } else {
        S3EndpointSecurity::HttpsOnly
    }
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
    if let Ok(value) = std::env::var("FILEFERRY_S3_ALLOW_INSECURE_HTTP") {
        command.env("FILEFERRY_S3_ALLOW_INSECURE_HTTP", value);
    }
    command
}

fn s3_provider_fileferry(
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
        .env("FILEFERRY_S3_SECRET_ACCESS_KEY", secret_access_key);
    if let Ok(value) = std::env::var("FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE") {
        command.env("FILEFERRY_S3_DISABLE_CONDITIONAL_CREATE", value);
    }
    if let Ok(value) = std::env::var("FILEFERRY_S3_ALLOW_INSECURE_HTTP") {
        command.env("FILEFERRY_S3_ALLOW_INSECURE_HTTP", value);
    }
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

fn run_s3_provider_json_command<const N: usize>(
    context: &S3LiveCommandContext<'_>,
    args: [&str; N],
    expected_code: i32,
) -> Vec<u8> {
    let output = s3_provider_fileferry(
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

fn run_s3_provider_jsonl_command<const N: usize>(
    context: &S3LiveCommandContext<'_>,
    args: [&str; N],
    expected_code: i32,
) -> Vec<u8> {
    run_s3_provider_json_command(context, args, expected_code)
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
    for value in sensitive_values {
        if value.is_empty() {
            continue;
        }
        assert!(
            !stdout.contains(value),
            "stdout leaked sensitive S3 test value\nstdout={redacted_stdout}"
        );
        assert!(
            !stderr.contains(value),
            "stderr leaked sensitive S3 test value\nstderr={redacted_stderr}"
        );
    }

    output.stdout
}

fn parse_jsonl_events(output: &[u8]) -> Vec<Value> {
    std::str::from_utf8(output)
        .expect("jsonl utf8")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("jsonl event"))
        .collect()
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
    let cleanup_config = S3StoreConfig::new_with_endpoint_security(
        bucket.to_owned(),
        region.to_owned(),
        endpoint.to_owned(),
        access_key_id.to_owned(),
        secret_access_key.to_owned(),
        ObjectKeyPrefix::new(repo_prefix.to_owned()).expect("s3 cleanup prefix"),
        s3_endpoint_security_from_env(),
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
fn find_searches_snapshot_contents_by_path_name_glob_and_tag() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::create_dir(source.join("docs")).expect("create docs");
    fs::write(source.join("docs").join("report.txt"), b"report").expect("write report");
    fs::write(source.join("docs").join("notes.md"), b"notes").expect("write notes");
    let first_backup = backup_source_with_tags(&repo_url, passphrase, &source, &["alpha"]);
    let first_snapshot_id = first_backup["data"]["snapshot_id"]
        .as_str()
        .expect("first snapshot id")
        .to_owned();

    fs::write(source.join("docs").join("summary.txt"), b"summary").expect("write summary");
    let second_backup = backup_source_with_tags(&repo_url, passphrase, &source, &["beta"]);
    let second_snapshot_id = second_backup["data"]["snapshot_id"]
        .as_str()
        .expect("second snapshot id")
        .to_owned();

    let name_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "find",
            "--name",
            "summary.txt",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let name_find: Value = serde_json::from_slice(&name_output).expect("find name json");
    assert_eq!(name_find["command"], "find");
    assert_eq!(name_find["status"], "success");
    assert_eq!(name_find["data"]["snapshots_searched"], 1);
    assert_eq!(name_find["data"]["matches_count"], 1);
    assert_eq!(
        name_find["data"]["matches"][0]["snapshot_id"],
        second_snapshot_id
    );
    assert_eq!(
        name_find["data"]["matches"][0]["path"],
        platform_relative_path("docs/summary.txt")
    );
    assert_eq!(name_find["data"]["matches"][0]["kind"], "regular_file");
    assert_eq!(name_find["data"]["matches"][0]["size_bytes"], 7);
    assert_eq!(
        name_find["data"]["matches"][0]["match_reasons"],
        json!(["name"])
    );

    let glob_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "find",
            "--all",
            "--glob",
            "docs/*.txt",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let glob_find: Value = serde_json::from_slice(&glob_output).expect("find glob json");
    assert_eq!(glob_find["data"]["snapshots_searched"], 2);
    assert_eq!(glob_find["data"]["matches_count"], 3);
    let glob_paths = glob_find["data"]["matches"]
        .as_array()
        .expect("matches array")
        .iter()
        .map(|entry| {
            (
                entry["snapshot_id"]
                    .as_str()
                    .expect("snapshot id")
                    .to_owned(),
                entry["path"].as_str().expect("path").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert!(glob_paths.contains(&(
        first_snapshot_id.clone(),
        platform_relative_path("docs/report.txt")
    )));
    assert!(glob_paths.contains(&(
        second_snapshot_id.clone(),
        platform_relative_path("docs/report.txt")
    )));
    assert!(glob_paths.contains(&(
        second_snapshot_id.clone(),
        platform_relative_path("docs/summary.txt")
    )));

    let path_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "find",
            "--snapshot",
            &second_snapshot_id,
            "--path",
            "docs",
        ])
        .assert()
        .success()
        .stdout(predicates::str::contains(format!(
            "file\t7\tpath\t{}",
            platform_relative_path("docs/summary.txt")
        )))
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let path_text = String::from_utf8(path_output).expect("path find utf8");
    assert!(path_text.contains(&format!("dir\t-\tpath\t{}", platform_relative_path("docs"))));
    assert!(path_text.contains(&format!(
        "file\t6\tpath\t{}",
        platform_relative_path("docs/report.txt")
    )));
    assert!(path_text.contains(&format!(
        "file\t5\tpath\t{}",
        platform_relative_path("docs/notes.md")
    )));

    let tag_jsonl_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "find", "--tag", "alpha"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = tag_jsonl_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let started: Value = serde_json::from_slice(lines[0]).expect("find started");
    let completed: Value = serde_json::from_slice(lines[1]).expect("find completed");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "find");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["command"], "find");
    assert_eq!(completed["data"]["snapshots_searched"], 1);
    assert_eq!(completed["data"]["matches_count"], 3);
    assert!(
        completed["data"]["matches"]
            .as_array()
            .expect("tag matches")
            .iter()
            .all(|entry| entry["snapshot_id"] == first_snapshot_id
                && entry["match_reasons"] == json!(["tag"]))
    );
}

#[test]
fn find_no_match_and_wrong_password_are_structured_and_redacted() {
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
        .args([
            "--repo",
            &repo_url,
            "--json",
            "find",
            "--name",
            "missing.txt",
        ])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let no_match: Value = serde_json::from_slice(&no_match_output).expect("no match json");
    assert_eq!(no_match["command"], "find");
    assert_eq!(no_match["status"], "failure");
    assert_eq!(no_match["data"]["code"], "find_no_matches");
    assert_eq!(no_match["data"]["exit_code"], 7);

    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-find-passphrase-canary")
        .args([
            "--repo",
            &repo_url,
            "--json",
            "find",
            "--name",
            "sample.txt",
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
    assert!(!wrong_text.contains("wrong-find-passphrase-canary"));
}

#[test]
fn diff_compares_snapshot_manifests_with_scriptable_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("changed.txt"), b"version-one").expect("write changed");
    fs::write(source.join("removed.txt"), b"removed").expect("write removed");
    fs::write(source.join("same.txt"), b"same").expect("write same");
    let first_backup = backup_source_with_tags(&repo_url, passphrase, &source, &["before"]);
    let first_snapshot_id = first_backup["data"]["snapshot_id"]
        .as_str()
        .expect("first snapshot id")
        .to_owned();

    fs::write(source.join("changed.txt"), b"version-two").expect("modify changed");
    fs::remove_file(source.join("removed.txt")).expect("remove file");
    fs::write(source.join("added.txt"), b"added").expect("write added");
    let second_backup = backup_source_with_tags(&repo_url, passphrase, &source, &["after"]);
    let second_snapshot_id = second_backup["data"]["snapshot_id"]
        .as_str()
        .expect("second snapshot id")
        .to_owned();

    let diff_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "diff",
            "--from-snapshot",
            &first_snapshot_id,
            "--to-snapshot",
            &second_snapshot_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let diff: Value = serde_json::from_slice(&diff_output).expect("diff json");
    assert_eq!(diff["command"], "diff");
    assert_eq!(diff["status"], "success");
    assert_eq!(diff["data"]["from_snapshot_id"], first_snapshot_id);
    assert_eq!(diff["data"]["to_snapshot_id"], second_snapshot_id);
    assert_eq!(diff["data"]["path_scopes"], json!(["."]));
    assert_eq!(diff["data"]["added_count"], 1);
    assert_eq!(diff["data"]["removed_count"], 1);
    assert_eq!(diff["data"]["changed_count"], 1);
    assert_eq!(diff["data"]["unchanged_count"], 1);

    let entries = diff["data"]["entries"].as_array().expect("diff entries");
    let changed = entries
        .iter()
        .find(|entry| entry["path"] == platform_relative_path("changed.txt"))
        .expect("changed entry");
    assert_eq!(changed["status"], "changed");
    assert_eq!(changed["content_changed"], true);
    assert_eq!(changed["from"]["kind"], "regular_file");
    assert_eq!(changed["to"]["kind"], "regular_file");
    assert_eq!(changed["from"]["size_bytes"], 11);
    assert_eq!(changed["to"]["size_bytes"], 11);
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry["path"] == platform_relative_path("added.txt"))
            .expect("added entry")["status"],
        "added"
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry["path"] == platform_relative_path("removed.txt"))
            .expect("removed entry")["status"],
        "removed"
    );
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry["path"] == platform_relative_path("same.txt"))
            .expect("same entry")["status"],
        "unchanged"
    );

    let scoped_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "diff",
            "--from-tag",
            "before",
            "--to-tag",
            "after",
            "--path",
            "changed.txt",
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let scoped: Value = serde_json::from_slice(&scoped_output).expect("scoped diff json");
    assert_eq!(
        scoped["data"]["path_scopes"],
        json!([platform_relative_path("changed.txt")])
    );
    assert_eq!(scoped["data"]["added_count"], 0);
    assert_eq!(scoped["data"]["removed_count"], 0);
    assert_eq!(scoped["data"]["changed_count"], 1);
    assert_eq!(scoped["data"]["unchanged_count"], 0);

    fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "diff",
            "--from-snapshot",
            &first_snapshot_id,
            "--to-snapshot",
            &second_snapshot_id,
        ])
        .assert()
        .success()
        .stdout(
            predicates::str::contains("added=1 removed=1 changed=1 unchanged=1").and(
                predicates::str::contains("changed\tfile\t11\tcontent_changed=true"),
            ),
        )
        .stderr("");

    let jsonl_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "diff",
            "--from-snapshot",
            &first_snapshot_id,
            "--to-snapshot",
            &second_snapshot_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = jsonl_output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(lines.len(), 2);
    let started: Value = serde_json::from_slice(lines[0]).expect("diff started");
    let completed: Value = serde_json::from_slice(lines[1]).expect("diff completed");
    assert_eq!(started["event"], "command_started");
    assert_eq!(started["command"], "diff");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["command"], "diff");
    assert_eq!(completed["data"]["changed_count"], 1);
}

#[test]
fn diff_no_difference_missing_path_wrong_password_and_missing_manifest_are_structured() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("same.txt"), b"same").expect("write same");
    let backup = backup_source(&repo_url, passphrase, &source);
    let snapshot_id = backup["data"]["snapshot_id"]
        .as_str()
        .expect("snapshot id")
        .to_owned();

    let no_diff_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "diff",
            "--from-snapshot",
            &snapshot_id,
            "--to-snapshot",
            &snapshot_id,
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let no_diff: Value = serde_json::from_slice(&no_diff_output).expect("no diff json");
    assert_eq!(no_diff["data"]["added_count"], 0);
    assert_eq!(no_diff["data"]["removed_count"], 0);
    assert_eq!(no_diff["data"]["changed_count"], 0);
    assert_eq!(no_diff["data"]["unchanged_count"], 1);

    let missing_path_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "diff",
            "--from-snapshot",
            &snapshot_id,
            "--to-snapshot",
            &snapshot_id,
            "--path",
            "missing.txt",
        ])
        .assert()
        .code(7)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing_path: Value =
        serde_json::from_slice(&missing_path_output).expect("missing path json");
    assert_eq!(missing_path["command"], "diff");
    assert_eq!(missing_path["status"], "failure");
    assert_eq!(missing_path["data"]["code"], "snapshot_path_not_found");
    assert_eq!(missing_path["data"]["exit_code"], 7);

    let wrong_output = fileferry()
        .env("FILEFERRY_PASSWORD", "wrong-diff-passphrase-canary")
        .args([
            "--repo",
            &repo_url,
            "--json",
            "diff",
            "--from-snapshot",
            &snapshot_id,
            "--to-snapshot",
            &snapshot_id,
        ])
        .assert()
        .code(4)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let wrong_text = String::from_utf8(wrong_output.clone()).expect("wrong diff utf8");
    let wrong: Value = serde_json::from_slice(&wrong_output).expect("wrong diff json");
    assert_eq!(wrong["data"]["code"], "repository_unlock_failed");
    assert_eq!(wrong["data"]["exit_code"], 4);
    assert!(!wrong_text.contains("wrong-diff-passphrase-canary"));

    let manifest_path = find_first_file(repo.join("objects/manifest"));
    fs::remove_file(manifest_path).expect("remove manifest");
    let missing_manifest_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--json",
            "diff",
            "--from-snapshot",
            &snapshot_id,
            "--to-snapshot",
            &snapshot_id,
        ])
        .assert()
        .code(6)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let missing_manifest: Value =
        serde_json::from_slice(&missing_manifest_output).expect("missing manifest json");
    assert_eq!(
        missing_manifest["data"]["code"],
        "repository_referenced_object_missing"
    );
    assert_eq!(missing_manifest["data"]["exit_code"], 6);
}

#[test]
fn backup_active_lease_has_stable_locked_exit_code_and_writes_no_snapshot() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "test-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write sample");

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let store = LocalStore::new(&repo);
    let opened = runtime
        .block_on(open_repository(&store, &SecretString::from(passphrase)))
        .expect("open repository");
    let pipeline =
        BackupPipeline::new(BackupPipelineConfig::new(opened.repository_id)).expect("pipeline");
    runtime
        .block_on(pipeline.write_repository_lease_state(
            &store,
            &opened.master_key,
            RepositoryLeaseStateRequest {
                lease_id: "de".repeat(32),
                writer_id: "ef".repeat(32),
                command_kind: RepositoryLeaseCommandKind::Prune,
                acquired_at_unix_seconds: 1,
                expires_at_unix_seconds: 4_000_000_000,
            },
        ))
        .expect("write active lease");

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
        .code(3)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let failed: Value = serde_json::from_slice(&output).expect("failure json");
    assert_eq!(failed["status"], "failure");
    assert_eq!(failed["data"]["code"], "repository_locked");
    assert_eq!(failed["data"]["exit_code"], 3);

    for relative in [
        "commits",
        "objects/chunk",
        "objects/index",
        "objects/manifest",
    ] {
        assert!(
            !repo.join(relative).exists(),
            "backup wrote snapshot objects under {relative}"
        );
    }
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
