use assert_cmd::Command;
use serde_json::Value;
use std::{
    fs,
    path::Path,
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
        .args(["--repo", repo_url, "--json", "init"])
        .assert()
        .success()
        .stderr("");
}

fn run_json(args: &[&str], repo_url: &str, passphrase: &str) -> Value {
    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", repo_url, "--json"])
        .args(args)
        .assert()
        .success()
        .stderr("")
        .get_output()
        .stdout
        .clone();

    serde_json::from_slice(&output).expect("json output")
}

fn run_restore_json(args: &[&str], repo_url: &str, passphrase: &str) -> Value {
    let output = fileferry()
        .env("FILEFERRY_PASSWORD", passphrase)
        .args(["--repo", repo_url, "--json", "restore"])
        .args(args)
        .assert()
        .code(10)
        .stderr("")
        .get_output()
        .stdout
        .clone();

    serde_json::from_slice(&output).expect("restore json")
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
        .map(|index| ((index * 31 + seed * 17 + index / 5) % 251) as u8)
        .collect()
}

#[cfg(unix)]
fn expected_symlink_count() -> u64 {
    1
}

#[cfg(not(unix))]
fn expected_symlink_count() -> u64 {
    0
}

#[test]
fn local_restore_release_drill_restores_real_snapshots_and_checks_repository() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repo = temp.path().join("repo");
    let repo_url = repo.display().to_string();
    let passphrase = "local-release-drill-passphrase";
    init_repo(&repo_url, passphrase);

    let source = temp.path().join("source");
    fs::create_dir_all(source.join("docs/archive/empty")).expect("create source tree");
    fs::write(source.join("docs/report.txt"), b"release drill report\n").expect("write report");
    fs::write(source.join("docs/blob.bin"), patterned_bytes(7, 1_200_000)).expect("write blob");
    let report_modified = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    set_modified_time(&source.join("docs/report.txt"), report_modified);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("report.txt", source.join("docs/report.link"))
            .expect("create symlink");
    }

    let backup = run_json(
        &[
            "backup",
            "--tag",
            "release-drill",
            source.to_str().expect("source path"),
        ],
        &repo_url,
        passphrase,
    );
    assert_eq!(backup["command"], "backup");
    assert_eq!(backup["status"], "success");
    let first_snapshot_id = backup["data"]["snapshot_id"]
        .as_str()
        .expect("first snapshot id");

    let full_restore = temp.path().join("restore-full");
    let full_restore_json = run_restore_json(
        &[
            "--tag",
            "release-drill",
            full_restore.to_str().expect("full restore path"),
        ],
        &repo_url,
        passphrase,
    );
    assert_eq!(full_restore_json["status"], "success");
    assert_eq!(
        full_restore_json["data"]["snapshot_id"],
        serde_json::json!(first_snapshot_id)
    );
    assert_eq!(full_restore_json["data"]["files_written"], 2);
    assert_eq!(
        full_restore_json["data"]["symlinks_written"].as_u64(),
        Some(expected_symlink_count())
    );
    assert_eq!(full_restore_json["data"]["verified_files"], 2);
    assert_eq!(
        fs::read(full_restore.join("docs/report.txt")).expect("restored report"),
        b"release drill report\n"
    );
    assert_eq!(
        fs::metadata(full_restore.join("docs/report.txt"))
            .expect("restored report metadata")
            .modified()
            .expect("restored report modified time"),
        report_modified
    );
    assert_eq!(
        fs::read(full_restore.join("docs/blob.bin")).expect("restored blob"),
        patterned_bytes(7, 1_200_000)
    );
    assert!(full_restore.join("docs/archive/empty").is_dir());
    #[cfg(unix)]
    assert_eq!(
        fs::read_link(full_restore.join("docs/report.link")).expect("restored symlink"),
        Path::new("report.txt")
    );

    let path_restore = temp.path().join("restore-path");
    let path_restore_json = run_restore_json(
        &[
            "--snapshot",
            first_snapshot_id,
            "--path",
            "docs/report.txt",
            path_restore.to_str().expect("path restore path"),
        ],
        &repo_url,
        passphrase,
    );
    assert_eq!(path_restore_json["data"]["entries_selected"], 1);
    assert_eq!(path_restore_json["data"]["files_written"], 1);
    assert_eq!(path_restore_json["data"]["verified_files"], 1);
    assert_eq!(
        fs::read(path_restore.join("docs/report.txt")).expect("path restored report"),
        b"release drill report\n"
    );
    assert!(!path_restore.join("docs/blob.bin").exists());

    let latest_source = temp.path().join("latest-source");
    fs::create_dir(&latest_source).expect("create latest source");
    fs::write(latest_source.join("latest.txt"), b"latest release drill\n").expect("write latest");
    let latest_backup = run_json(
        &[
            "backup",
            "--tag",
            "latest-drill",
            latest_source.to_str().expect("latest source path"),
        ],
        &repo_url,
        passphrase,
    );
    assert_ne!(
        latest_backup["data"]["snapshot_id"],
        serde_json::json!(first_snapshot_id)
    );

    let latest_restore = temp.path().join("restore-latest");
    let latest_restore_json = run_restore_json(
        &["--latest", latest_restore.to_str().expect("latest path")],
        &repo_url,
        passphrase,
    );
    assert_eq!(
        latest_restore_json["data"]["snapshot_id"],
        latest_backup["data"]["snapshot_id"]
    );
    assert_eq!(
        fs::read(latest_restore.join("latest.txt")).expect("latest restored file"),
        b"latest release drill\n"
    );
    assert!(!latest_restore.join("docs/report.txt").exists());

    let check = run_json(&["check"], &repo_url, passphrase);
    assert_eq!(check["command"], "check");
    assert_eq!(check["status"], "success");
    assert_eq!(check["data"]["read_data_mode"], "full");
    assert_eq!(check["data"]["errors"], serde_json::json!([]));
    assert_eq!(check["data"]["warnings"], serde_json::json!([]));
    assert!(
        check["data"]["metadata_objects_checked"]
            .as_u64()
            .expect("metadata objects")
            >= 6
    );
    assert!(
        check["data"]["chunk_objects_checked"]
            .as_u64()
            .expect("chunk objects")
            >= 2
    );
}
