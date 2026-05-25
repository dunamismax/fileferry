use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
#[cfg(unix)]
fn unix_installer_installs_archive_and_verifies_checksum() {
    let fixture = Fixture::new("unix-installs");
    let archive = fixture.release_archive("ferry", b"#!/bin/sh\necho ferry-test\n");
    fixture.write_checksums(&archive, None);

    let output = Command::new("sh")
        .arg(workspace_root().join("scripts/install.sh"))
        .arg("--archive")
        .arg(&archive)
        .arg("--install-dir")
        .arg(fixture.path("bin"))
        .output()
        .expect("run install.sh");

    assert_success(&output);

    let installed = fixture.path("bin/ferry");
    assert_eq!(
        fs::read_to_string(&installed).expect("read installed ferry"),
        "#!/bin/sh\necho ferry-test\n"
    );

    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(&installed)
        .expect("stat installed ferry")
        .permissions()
        .mode();
    assert_ne!(mode & 0o111, 0, "installed ferry should be executable");
}

#[test]
#[cfg(unix)]
fn unix_installer_dry_run_does_not_write_binary() {
    let fixture = Fixture::new("unix-dry-run");
    let archive = fixture.release_archive("ferry", b"#!/bin/sh\necho ferry-test\n");
    fixture.write_checksums(&archive, None);

    let output = Command::new("sh")
        .arg(workspace_root().join("scripts/install.sh"))
        .arg("--archive")
        .arg(&archive)
        .arg("--install-dir")
        .arg(fixture.path("bin"))
        .arg("--dry-run")
        .output()
        .expect("run install.sh dry-run");

    assert_success(&output);
    assert!(!fixture.path("bin/ferry").exists());
}

#[test]
#[cfg(unix)]
fn unix_installer_rejects_checksum_mismatch() {
    let fixture = Fixture::new("unix-checksum-mismatch");
    let archive = fixture.release_archive("ferry", b"#!/bin/sh\necho ferry-test\n");
    fixture.write_checksums(
        &archive,
        Some("0000000000000000000000000000000000000000000000000000000000000000"),
    );

    let output = Command::new("sh")
        .arg(workspace_root().join("scripts/install.sh"))
        .arg("--archive")
        .arg(&archive)
        .arg("--install-dir")
        .arg(fixture.path("bin"))
        .output()
        .expect("run install.sh with bad checksum");

    assert_failure_contains(&output, "checksum mismatch");
    assert!(!fixture.path("bin/ferry").exists());
}

#[test]
fn powershell_installer_installs_archive_and_verifies_checksum() {
    let Some(pwsh) = pwsh() else {
        eprintln!("skipping PowerShell installer test because pwsh is not installed");
        return;
    };

    let fixture = Fixture::new("powershell-installs");
    let binary_name = if cfg!(windows) { "ferry.exe" } else { "ferry" };
    let archive = fixture.release_archive(binary_name, b"ferry-test\n");
    fixture.write_checksums(&archive, None);

    let output = Command::new(pwsh)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(workspace_root().join("scripts/install.ps1"))
        .arg("-Archive")
        .arg(&archive)
        .arg("-InstallDir")
        .arg(fixture.path("bin"))
        .output()
        .expect("run install.ps1");

    assert_success(&output);

    let installed = fixture.path(format!("bin/{binary_name}"));
    assert_eq!(
        fs::read_to_string(installed).expect("read installed ferry"),
        "ferry-test\n"
    );
}

#[test]
fn powershell_installer_rejects_checksum_mismatch() {
    let Some(pwsh) = pwsh() else {
        eprintln!("skipping PowerShell checksum test because pwsh is not installed");
        return;
    };

    let fixture = Fixture::new("powershell-checksum-mismatch");
    let binary_name = if cfg!(windows) { "ferry.exe" } else { "ferry" };
    let archive = fixture.release_archive(binary_name, b"ferry-test\n");
    fixture.write_checksums(
        &archive,
        Some("0000000000000000000000000000000000000000000000000000000000000000"),
    );

    let output = Command::new(pwsh)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
        .arg(workspace_root().join("scripts/install.ps1"))
        .arg("-Archive")
        .arg(&archive)
        .arg("-InstallDir")
        .arg(fixture.path("bin"))
        .output()
        .expect("run install.ps1 with bad checksum");

    assert_failure_contains(&output, "checksum mismatch");
    assert!(!fixture.path(format!("bin/{binary_name}")).exists());
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = env::temp_dir().join(format!(
            "fileferry-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&root).expect("create fixture root");
        Self { root }
    }

    fn path(&self, relative: impl AsRef<Path>) -> PathBuf {
        self.root.join(relative)
    }

    fn release_archive(&self, binary_name: &str, binary_contents: &[u8]) -> PathBuf {
        let package_dir = self.path("stage/fileferry-1.0.0-test-target");
        fs::create_dir_all(&package_dir).expect("create package dir");
        let binary = package_dir.join(binary_name);
        fs::write(&binary, binary_contents).expect("write binary");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&binary).expect("stat binary").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&binary, permissions).expect("chmod binary");
        }

        let archive = self.path("fileferry-1.0.0-test-target.tar.gz");
        let output = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(self.path("stage"))
            .arg("fileferry-1.0.0-test-target")
            .output()
            .expect("create test archive with tar");
        assert_success(&output);

        archive
    }

    fn write_checksums(&self, archive: &Path, hash_override: Option<&str>) {
        let hash = hash_override
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| sha256_file(archive));
        let archive_name = archive
            .file_name()
            .and_then(|name| name.to_str())
            .expect("archive has UTF-8 filename");
        fs::write(self.path("SHA256SUMS"), format!("{hash}  {archive_name}\n"))
            .expect("write checksum file");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives under workspace root")
        .to_path_buf()
}

fn pwsh() -> Option<&'static str> {
    Command::new("pwsh")
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|_| "pwsh")
}

fn sha256_file(path: &Path) -> String {
    let bytes = fs::read(path).expect("read archive for hashing");
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure_contains(output: &Output, expected: &str) {
    assert!(
        !output.status.success(),
        "command should have failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains(expected),
        "expected output to contain {expected:?}, got:\n{combined}"
    );
}
