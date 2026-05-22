use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;

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

fn jsonl_events(output: &[u8]) -> Vec<Value> {
    output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).expect("valid jsonl event"))
        .collect()
}

fn event_names(events: &[Value]) -> Vec<&str> {
    events
        .iter()
        .map(|event| event["event"].as_str().expect("event name"))
        .collect()
}

fn progress_phases(events: &[Value]) -> Vec<&str> {
    events
        .iter()
        .filter(|event| event["event"] == "progress")
        .map(|event| event["data"]["phase"].as_str().expect("progress phase"))
        .collect()
}

#[test]
fn top_level_help_lists_stable_global_flags_and_commands() {
    let mut command = fileferry();

    command
        .arg("--help")
        .assert()
        .success()
        .stdout(
            predicates::str::contains("Encrypted backups. Same everywhere.")
                .and(predicates::str::contains("--repo <REPO>"))
                .and(predicates::str::contains("--profile <PROFILE>"))
                .and(predicates::str::contains("--config <CONFIG>"))
                .and(predicates::str::contains("--json"))
                .and(predicates::str::contains("--jsonl"))
                .and(predicates::str::contains("completion"))
                .and(predicates::str::contains("init"))
                .and(predicates::str::contains("backup"))
                .and(predicates::str::contains("restore"))
                .and(predicates::str::contains("snapshots"))
                .and(predicates::str::contains("ls"))
                .and(predicates::str::contains("check"))
                .and(predicates::str::contains("forget"))
                .and(predicates::str::contains("version")),
        )
        .stderr("");
}

#[test]
fn output_mode_flags_conflict() {
    let mut command = fileferry();

    command
        .args(["--json", "--jsonl", "version"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("cannot be used with"));
}

#[test]
fn unknown_argument_exits_with_usage_error() {
    let mut command = fileferry();

    command
        .args(["version", "--not-a-real-flag"])
        .assert()
        .code(2)
        .stderr(predicates::str::contains("unexpected argument"));
}

#[test]
fn machine_failure_envelopes_keep_streams_separated_and_ordered() {
    let json_output = fileferry()
        .args(["--json", "snapshots"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let json: Value = serde_json::from_slice(&json_output).expect("failure json");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "snapshots");
    assert_eq!(json["status"], "failure");
    assert_eq!(json["data"]["code"], "repository_url_missing");
    assert_eq!(json["data"]["exit_code"], 2);
    assert_eq!(json["data"]["retryable"], false);
    assert!(json["data"]["path"].is_null());
    assert!(json["data"]["object_key"].is_null());

    let jsonl_output = fileferry()
        .args(["--jsonl", "snapshots"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let events = jsonl_events(&jsonl_output);
    assert_eq!(event_names(&events), ["command_started", "command_failed"]);
    assert_eq!(events[0]["schema_version"], 1);
    assert_eq!(events[0]["command"], "snapshots");
    assert_eq!(events[0]["status"], "started");
    assert!(events[0]["data"].is_null());
    assert_eq!(events[1]["command"], "snapshots");
    assert_eq!(events[1]["status"], "failure");
    assert_eq!(events[1]["data"]["code"], "repository_url_missing");
    assert_eq!(events[1]["data"]["exit_code"], 2);

    fileferry()
        .arg("snapshots")
        .assert()
        .code(2)
        .stdout("")
        .stderr(predicates::str::contains("repository URL is required"));
}

#[test]
fn secret_bearing_password_env_names_are_not_emitted_in_runtime_failures() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo_url = temp.path().join("repo").display().to_string();

    let json_output = fileferry()
        .args(["--repo", &repo_url, "--json", "init"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let json_text = String::from_utf8(json_output.clone()).expect("json utf8");
    let json: Value = serde_json::from_slice(&json_output).expect("failure json");
    assert_eq!(json["data"]["code"], "repository_password_missing");
    assert!(!json_text.contains("FILEFERRY_PASSWORD"));
    assert!(!json_text.contains("FILEFERRY_PASSWORD_FILE"));

    let jsonl_output = fileferry()
        .env("FILEFERRY_PASSWORD", "current-passphrase-canary")
        .args(["--repo", &repo_url, "--jsonl", "key", "add"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let jsonl_text = String::from_utf8(jsonl_output.clone()).expect("jsonl utf8");
    let events = jsonl_events(&jsonl_output);
    assert_eq!(events[1]["data"]["code"], "repository_new_password_missing");
    assert!(!jsonl_text.contains("current-passphrase-canary"));
    assert!(!jsonl_text.contains("FILEFERRY_PASSWORD"));
    assert!(!jsonl_text.contains("FILEFERRY_NEW_PASSWORD"));
    assert!(!jsonl_text.contains("FILEFERRY_NEW_PASSWORD_FILE"));

    fileferry()
        .args(["--repo", &repo_url, "init"])
        .assert()
        .code(2)
        .stdout("")
        .stderr(
            predicates::str::contains("repository passphrase is required")
                .and(predicates::str::contains("FILEFERRY_PASSWORD").not()),
        );
}

#[test]
fn config_parse_failures_redact_secret_url_fragments_in_machine_output() {
    let temp = tempfile::tempdir().expect("tempdir");
    let config = temp.path().join("fileferry.toml");
    fs::write(
        &config,
        r#"
[repository]
url = "https://user:secret@example.com/repo?token=sensitive
"#,
    )
    .expect("write config");
    let config_path = config.to_str().expect("config path");

    let json_output = fileferry()
        .args(["--config", config_path, "--json", "version"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let json_text = String::from_utf8(json_output.clone()).expect("json utf8");
    let json: Value = serde_json::from_slice(&json_output).expect("failure json");
    assert_eq!(json["data"]["code"], "config_parse_failed");
    assert!(json_text.contains("https://<redacted>@example.com/repo?<redacted>"));
    assert!(!json_text.contains("user:secret"));
    assert!(!json_text.contains("token=sensitive"));

    let jsonl_output = fileferry()
        .args(["--config", config_path, "--jsonl", "version"])
        .assert()
        .code(2)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let jsonl_text = String::from_utf8(jsonl_output).expect("jsonl utf8");
    assert!(jsonl_text.contains("https://<redacted>@example.com/repo?<redacted>"));
    assert!(!jsonl_text.contains("user:secret"));
    assert!(!jsonl_text.contains("token=sensitive"));
}

#[test]
fn local_repository_jsonl_event_order_matches_contract() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let source = temp.path().join("source");
    let restore = temp.path().join("restore");
    let passphrase = "test-passphrase";

    fs::create_dir(&source).expect("create source");
    fs::write(source.join("sample.txt"), b"sample").expect("write source file");

    let init_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "init"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let init_events = jsonl_events(&init_output);
    assert_eq!(
        event_names(&init_events),
        ["command_started", "command_completed"]
    );
    assert_eq!(init_events[1]["data"]["backend"], "local");

    let backup_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "backup",
            "--tag",
            "contract",
            source.to_str().expect("source path"),
        ])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let backup_events = jsonl_events(&backup_output);
    assert_eq!(backup_events.first().unwrap()["event"], "command_started");
    assert_eq!(backup_events.last().unwrap()["event"], "command_completed");
    assert_eq!(
        progress_phases(&backup_events),
        [
            "walk_sources",
            "plan_chunks",
            "write_chunks",
            "write_index",
            "write_manifest",
            "write_commit",
            "complete"
        ]
    );

    let snapshots_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "snapshots"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let snapshots_events = jsonl_events(&snapshots_output);
    assert_eq!(
        event_names(&snapshots_events),
        ["command_started", "command_completed"]
    );
    assert_eq!(
        snapshots_events[1]["data"]["snapshots"]
            .as_array()
            .expect("snapshots array")
            .len(),
        1
    );

    let restore_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args([
            "--repo",
            &repo_url,
            "--jsonl",
            "restore",
            restore.to_str().expect("restore path"),
        ])
        .assert()
        .code(10)
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let restore_events = jsonl_events(&restore_output);
    assert_eq!(restore_events.first().unwrap()["event"], "command_started");
    assert_eq!(restore_events.last().unwrap()["event"], "command_completed");
    assert_eq!(
        progress_phases(&restore_events),
        [
            "load_manifest",
            "read_chunks",
            "write_entries",
            "apply_metadata",
            "verify",
            "complete"
        ]
    );
    assert!(
        restore_events
            .iter()
            .any(|event| event["event"] == "warning")
    );
    assert_eq!(restore_events.last().unwrap()["data"]["files_written"], 1);

    let check_output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", &repo_url, "--jsonl", "check"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let check_events = jsonl_events(&check_output);
    assert_eq!(check_events.first().unwrap()["event"], "command_started");
    assert_eq!(check_events.last().unwrap()["event"], "command_completed");
    assert_eq!(
        progress_phases(&check_events),
        [
            "load_commits",
            "verify_metadata",
            "verify_indexes",
            "read_data",
            "complete"
        ]
    );
}
