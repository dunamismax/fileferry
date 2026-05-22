use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    if let Err(error) = run(env::args_os().skip(1).collect()) {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run(args: Vec<OsString>) -> Result<(), String> {
    let Some(command) = args.first().and_then(|arg| arg.to_str()) else {
        return Err(usage());
    };

    match command {
        "release-package" => release_package(ReleasePackageOptions::parse(&args[1..])?),
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{}", usage())),
    }
}

fn usage() -> String {
    "usage: cargo run -p xtask -- release-package [--target TRIPLE] [--out-dir DIR] [--auditable] [--sbom] [--sign] [--skip-build] [--skip-smoke]".to_string()
}

#[derive(Debug, Default)]
struct ReleasePackageOptions {
    target: Option<String>,
    out_dir: Option<PathBuf>,
    auditable: bool,
    sbom: bool,
    sign: bool,
    skip_build: bool,
    skip_smoke: bool,
}

impl ReleasePackageOptions {
    fn parse(args: &[OsString]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut index = 0;

        while index < args.len() {
            let arg = args[index]
                .to_str()
                .ok_or_else(|| "arguments must be valid UTF-8".to_string())?;
            match arg {
                "--target" => {
                    index += 1;
                    options.target = Some(value(args, index, "--target")?);
                }
                "--out-dir" => {
                    index += 1;
                    options.out_dir = Some(PathBuf::from(value(args, index, "--out-dir")?));
                }
                "--auditable" => options.auditable = true,
                "--sbom" => options.sbom = true,
                "--sign" => options.sign = true,
                "--skip-build" => options.skip_build = true,
                "--skip-smoke" => options.skip_smoke = true,
                "--help" | "-h" => return Err(usage()),
                other => return Err(format!("unknown release-package option `{other}`")),
            }
            index += 1;
        }

        if options.auditable && options.skip_build {
            return Err("--auditable cannot be verified when --skip-build is set".to_string());
        }

        Ok(options)
    }
}

fn value(args: &[OsString], index: usize, flag: &str) -> Result<String, String> {
    args.get(index)
        .and_then(|arg| arg.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn release_package(options: ReleasePackageOptions) -> Result<(), String> {
    let workspace = env::current_dir().map_err(|error| format!("read current dir: {error}"))?;
    let host = host_triple()?;
    let target = options.target.unwrap_or_else(|| host.clone());
    let out_dir = options
        .out_dir
        .unwrap_or_else(|| workspace.join("target").join("release-artifacts"));
    let version = package_version(&workspace)?;
    let commit = git_commit(&workspace).unwrap_or_else(|_| "unknown".to_string());
    let binary_name = if target.contains("windows") {
        "ferry.exe"
    } else {
        "ferry"
    };

    if !options.skip_build {
        build_release(&target, options.auditable)?;
    }

    let binary = release_binary_path(&workspace, &target, &host, binary_name);
    if !binary.is_file() {
        return Err(format!(
            "release binary was not found at {}",
            binary.display()
        ));
    }

    fs::create_dir_all(&out_dir)
        .map_err(|error| format!("create artifact dir {}: {error}", out_dir.display()))?;

    let installers = copy_installers(&workspace, &out_dir)?;
    let artifact_stem = format!("fileferry-{version}-{target}");
    let stage_root = out_dir.join("stage");
    let stage_dir = stage_root.join(&artifact_stem);
    if stage_dir.exists() {
        fs::remove_dir_all(&stage_dir)
            .map_err(|error| format!("remove stale stage dir {}: {error}", stage_dir.display()))?;
    }
    fs::create_dir_all(&stage_dir)
        .map_err(|error| format!("create stage dir {}: {error}", stage_dir.display()))?;
    fs::copy(&binary, stage_dir.join(binary_name))
        .map_err(|error| format!("stage binary {}: {error}", binary.display()))?;
    fs::copy(workspace.join("README.md"), stage_dir.join("README.md"))
        .map_err(|error| format!("stage README.md: {error}"))?;
    fs::copy(workspace.join("LICENSE"), stage_dir.join("LICENSE"))
        .map_err(|error| format!("stage LICENSE: {error}"))?;

    let archive = out_dir.join(format!("{artifact_stem}.tar.gz"));
    create_tar_gz(&stage_root, &artifact_stem, &archive)?;

    let smoke_test = if options.skip_smoke || target != host {
        None
    } else {
        Some(smoke_test_binary(&binary)?)
    };

    let sbom = if options.sbom {
        Some(generate_sbom(
            &workspace,
            &out_dir,
            &artifact_stem,
            &target,
        )?)
    } else {
        None
    };

    let manifest_path = out_dir.join(format!("{artifact_stem}.manifest.json"));
    let manifest = ReleaseManifest {
        schema_version: 1,
        package: "fileferry",
        binary: "ferry",
        version: &version,
        target: &target,
        commit: &commit,
        archive: file_name(&archive)?,
        installers: installers
            .iter()
            .map(|path| file_name(path))
            .collect::<Result<Vec<_>, _>>()?,
        auditable_build: options.auditable,
        sbom: sbom.as_ref().map(|path| file_name(path)).transpose()?,
        smoke_test,
    };
    write_json(&manifest_path, &manifest)?;

    let mut checksum_inputs = vec![archive.clone(), manifest_path.clone()];
    checksum_inputs.extend(installers);
    if let Some(sbom_path) = &sbom {
        checksum_inputs.push(sbom_path.clone());
    }
    let checksums = out_dir.join("SHA256SUMS");
    write_checksums(&checksums, &checksum_inputs)?;

    let signature_bundle = if options.sign {
        let bundle = out_dir.join("SHA256SUMS.sigstore.json");
        sign_checksums(&checksums, &bundle)?;
        Some(bundle)
    } else {
        None
    };

    fs::remove_dir_all(&stage_root)
        .map_err(|error| format!("remove stage dir {}: {error}", stage_root.display()))?;

    println!("archive: {}", archive.display());
    println!("checksums: {}", checksums.display());
    if let Some(sbom_path) = sbom {
        println!("sbom: {}", sbom_path.display());
    }
    println!("manifest: {}", manifest_path.display());
    if let Some(bundle) = signature_bundle {
        println!("signature: {}", bundle.display());
    }

    Ok(())
}

fn copy_installers(workspace: &Path, out_dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut installers = Vec::new();
    for name in ["install.sh", "install.ps1"] {
        let source = workspace.join("scripts").join(name);
        if !source.is_file() {
            return Err(format!("installer script is missing: {}", source.display()));
        }
        let destination = out_dir.join(name);
        fs::copy(&source, &destination).map_err(|error| {
            format!(
                "copy installer {} to {}: {error}",
                source.display(),
                destination.display()
            )
        })?;
        installers.push(destination);
    }
    Ok(installers)
}

fn build_release(target: &str, auditable: bool) -> Result<(), String> {
    let mut command = Command::new("cargo");
    if auditable {
        command.args(["auditable", "build"]);
    } else {
        command.arg("build");
    }
    command.args([
        "-p",
        "fileferry-cli",
        "--bin",
        "ferry",
        "--release",
        "--target",
        target,
    ]);
    run_command(&mut command, "build release ferry binary")
}

fn release_binary_path(workspace: &Path, target: &str, host: &str, binary_name: &str) -> PathBuf {
    let targeted = workspace
        .join("target")
        .join(target)
        .join("release")
        .join(binary_name);
    if targeted.exists() {
        return targeted;
    }
    if target == host {
        return workspace.join("target").join("release").join(binary_name);
    }
    targeted
}

fn host_triple() -> Result<String, String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|error| format!("run rustc -vV: {error}"))?;
    if !output.status.success() {
        return Err("rustc -vV failed".to_string());
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("rustc -vV output was not UTF-8: {error}"))?;
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(ToOwned::to_owned)
        .ok_or_else(|| "rustc -vV did not report a host triple".to_string())
}

fn package_version(workspace: &Path) -> Result<String, String> {
    let manifest = fs::read_to_string(workspace.join("crates/fileferry-cli/Cargo.toml"))
        .map_err(|error| format!("read fileferry-cli manifest: {error}"))?;
    let value: toml::Value = toml::from_str(&manifest)
        .map_err(|error| format!("parse fileferry-cli manifest: {error}"))?;
    value
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| "fileferry-cli package version is missing".to_string())
}

fn git_commit(workspace: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(workspace)
        .output()
        .map_err(|error| format!("run git rev-parse: {error}"))?;
    if !output.status.success() {
        return Err("git rev-parse HEAD failed".to_string());
    }
    String::from_utf8(output.stdout)
        .map(|stdout| stdout.trim().to_string())
        .map_err(|error| format!("git output was not UTF-8: {error}"))
}

fn create_tar_gz(stage_root: &Path, directory_name: &str, archive: &Path) -> Result<(), String> {
    let mut command = Command::new("tar");
    command
        .arg("-czf")
        .arg(archive)
        .arg("-C")
        .arg(stage_root)
        .arg(directory_name);
    run_command(&mut command, "create release archive")
}

fn smoke_test_binary(binary: &Path) -> Result<SmokeTest, String> {
    let output = Command::new(binary)
        .args(["version", "--json"])
        .output()
        .map_err(|error| format!("run smoke test {}: {error}", binary.display()))?;
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("smoke-test stdout was not UTF-8: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "smoke test failed with status {:?}: {stderr}",
            output.status.code()
        ));
    }
    if !stdout.contains("\"version\"") {
        return Err("smoke test did not emit version JSON".to_string());
    }
    Ok(SmokeTest {
        command: format!("{} version --json", binary.display()),
        exit_code: output.status.code().unwrap_or_default(),
        stdout,
    })
}

fn generate_sbom(
    workspace: &Path,
    out_dir: &Path,
    artifact_stem: &str,
    target: &str,
) -> Result<PathBuf, String> {
    cleanup_generated_sboms(workspace, target)?;
    let mut command = Command::new("cargo");
    command.args([
        "cyclonedx",
        "--format",
        "json",
        "--describe",
        "binaries",
        "--target",
        target,
        "--target-in-filename",
        "--spec-version",
        "1.5",
    ]);
    run_command(&mut command, "generate CycloneDX SBOM")?;

    let generated = workspace
        .join("crates")
        .join("fileferry-cli")
        .join(format!("ferry_bin_{target}.cdx.json"));
    if !generated.is_file() {
        return Err(format!(
            "cargo-cyclonedx did not create expected SBOM {}",
            generated.display()
        ));
    }
    let destination = out_dir.join(format!("{artifact_stem}.cdx.json"));
    fs::rename(&generated, &destination).map_err(|error| {
        format!(
            "move SBOM {} to {}: {error}",
            generated.display(),
            destination.display()
        )
    })?;
    cleanup_generated_sboms(workspace, target)?;
    Ok(destination)
}

fn cleanup_generated_sboms(workspace: &Path, target: &str) -> Result<(), String> {
    for path in [
        workspace
            .join("crates")
            .join("fileferry-cli")
            .join(format!("ferry_bin_{target}.cdx.json")),
        workspace
            .join("crates")
            .join("fileferry-web")
            .join(format!("fileferry-web_bin_{target}.cdx.json")),
        workspace
            .join("xtask")
            .join(format!("xtask_bin_{target}.cdx.json")),
    ] {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|error| format!("remove generated SBOM {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value)
        .map_err(|error| format!("serialize {}: {error}", path.display()))?;
    fs::write(path, format!("{json}\n"))
        .map_err(|error| format!("write {}: {error}", path.display()))
}

fn write_checksums(path: &Path, inputs: &[PathBuf]) -> Result<(), String> {
    let mut lines = Vec::with_capacity(inputs.len());
    for input in inputs {
        lines.push(format!("{}  {}", sha256_file(input)?, file_name(input)?));
    }
    lines.sort();
    fs::write(path, format!("{}\n", lines.join("\n")))
        .map_err(|error| format!("write {}: {error}", path.display()))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sign_checksums(checksums: &Path, bundle: &Path) -> Result<(), String> {
    let mut command = Command::new("cosign");
    command
        .arg("sign-blob")
        .arg(checksums)
        .arg("--bundle")
        .arg(bundle)
        .arg("--yes");
    run_command(&mut command, "sign checksum manifest with cosign")
}

fn run_command(command: &mut Command, label: &str) -> Result<(), String> {
    let status = command
        .stdin(Stdio::null())
        .status()
        .map_err(|error| format!("{label}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{label} failed with status {status}"))
    }
}

fn file_name(path: &Path) -> Result<String, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("path has no UTF-8 file name: {}", path.display()))
}

#[derive(Serialize)]
struct ReleaseManifest<'a> {
    schema_version: u8,
    package: &'a str,
    binary: &'a str,
    version: &'a str,
    target: &'a str,
    commit: &'a str,
    archive: String,
    installers: Vec<String>,
    auditable_build: bool,
    sbom: Option<String>,
    smoke_test: Option<SmokeTest>,
}

#[derive(Serialize)]
struct SmokeTest {
    command: String,
    exit_code: i32,
    stdout: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_package_options() {
        let args = [
            "--target",
            "x86_64-unknown-linux-gnu",
            "--out-dir",
            "target/release-artifacts",
            "--auditable",
            "--sbom",
            "--sign",
            "--skip-smoke",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();

        let options = ReleasePackageOptions::parse(&args).expect("options should parse");

        assert_eq!(options.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert_eq!(
            options.out_dir.as_deref(),
            Some(Path::new("target/release-artifacts"))
        );
        assert!(options.auditable);
        assert!(options.sbom);
        assert!(options.sign);
        assert!(options.skip_smoke);
    }

    #[test]
    fn rejects_auditable_skip_build_combination() {
        let args = ["--auditable", "--skip-build"]
            .into_iter()
            .map(OsString::from)
            .collect::<Vec<_>>();

        let error = ReleasePackageOptions::parse(&args).expect_err("combination should fail");

        assert!(error.contains("--auditable cannot be verified"));
    }

    #[test]
    fn sha256_file_hashes_file_contents() {
        let path = env::temp_dir().join(format!("fileferry-xtask-sha256-{}", std::process::id()));
        fs::write(&path, b"fileferry\n").expect("write temp file");

        let hash = sha256_file(&path).expect("hash file");

        fs::remove_file(&path).expect("remove temp file");
        assert_eq!(
            hash,
            "e49295bbb366266f68873753efba2d2e2b5185f261b5c1baf360e0490865b02d"
        );
    }
}
