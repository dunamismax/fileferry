use assert_cmd::Command;
use predicates::prelude::*;

fn fileferry() -> Command {
    let mut command = Command::cargo_bin("ferry").expect("ferry binary");
    for variable in [
        "FILEFERRY_CONFIG",
        "FILEFERRY_PROFILE",
        "FILEFERRY_REPOSITORY",
        "FILEFERRY_PASSWORD",
        "FILEFERRY_PASSWORD_FILE",
        "FILEFERRY_LOG",
    ] {
        command.env_remove(variable);
    }
    command
}

#[test]
fn version_subcommand_prints_human_version() {
    let mut command = fileferry();

    command
        .arg("version")
        .assert()
        .success()
        .stdout("ferry 0.0.0\n")
        .stderr("");
}

#[test]
fn version_subcommand_supports_json() {
    let mut command = fileferry();

    let output = command
        .args(["--json", "version"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "version");
    assert_eq!(json["status"], "success");
    assert_eq!(json["data"]["command"], "ferry");
    assert_eq!(json["data"]["version"], "0.0.0");
}

#[test]
fn version_json_mode_emits_one_stdout_document_and_no_progress() {
    let mut command = fileferry();

    let output = command
        .args(["--json", "version"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).expect("utf8 stdout");

    assert_eq!(stdout.matches("\"schema_version\"").count(), 1);
    assert_eq!(
        stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count(),
        9
    );
    assert!(!stdout.contains("command_started"));
    assert!(!stdout.contains("progress"));
}

#[test]
fn global_output_flags_work_after_subcommands() {
    let mut command = fileferry();

    let output = command
        .args(["version", "--json"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON");
    assert_eq!(json["command"], "version");
    assert_eq!(json["data"]["command"], "ferry");
    assert_eq!(json["data"]["version"], "0.0.0");
}

#[test]
fn version_subcommand_supports_jsonl_events() {
    let mut command = fileferry();

    let output = command
        .args(["--jsonl", "version"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    let lines: Vec<_> = output.split(|byte| *byte == b'\n').collect();

    assert_eq!(lines.len(), 3);
    let started: serde_json::Value = serde_json::from_slice(lines[0]).expect("start event");
    let completed: serde_json::Value = serde_json::from_slice(lines[1]).expect("complete event");
    assert_eq!(started["event"], "command_started");
    assert_eq!(completed["event"], "command_completed");
    assert_eq!(completed["data"]["version"], "0.0.0");
}

#[test]
fn version_jsonl_mode_emits_only_stdout_events_and_no_human_progress() {
    let mut command = fileferry();

    let output = command
        .args(["--jsonl", "version"])
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();

    for line in output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let event: serde_json::Value = serde_json::from_slice(line).expect("jsonl event");
        assert!(event.get("schema_version").is_some());
        assert_ne!(event["event"], "progress");
    }
}

#[test]
fn completion_subcommand_prints_shell_completion_data() {
    let mut command = fileferry();

    command
        .args(["completion", "bash"])
        .assert()
        .success()
        .stdout(predicates::str::contains("_ferry").and(predicates::str::contains("version")))
        .stderr("");
}

#[test]
fn completion_subcommand_does_not_require_repository_config() {
    let mut command = fileferry();

    command
        .env(
            "FILEFERRY_REPOSITORY",
            "https://user:secret@example.com/repo",
        )
        .args(["completion", "zsh"])
        .assert()
        .success()
        .stdout(predicates::str::contains("#compdef ferry"))
        .stderr("");
}

#[test]
fn invalid_repository_exits_with_usage_error_and_redacts_secret_url_parts() {
    let mut command = fileferry();

    command
        .args([
            "--repo",
            "https://user:secret@example.com/repo?token=sensitive",
            "version",
        ])
        .assert()
        .code(2)
        .stderr(
            predicates::str::contains("https://<redacted>@example.com/repo?<redacted>")
                .and(predicates::str::contains("secret").not())
                .and(predicates::str::contains("sensitive").not()),
        );
}
