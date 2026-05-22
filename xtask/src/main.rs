use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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
        "archive-smoke" => archive_smoke_command(ArchiveSmokeOptions::parse(&args[1..])?),
        "--help" | "-h" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        other => Err(format!("unknown command `{other}`\n\n{}", usage())),
    }
}

fn usage() -> String {
    [
        "usage:",
        "  cargo run -p xtask -- release-package [--target TRIPLE] [--out-dir DIR] [--auditable] [--sbom] [--sign] [--skip-build] [--skip-smoke]",
        "  cargo run -p xtask -- archive-smoke --archive FILE [--checksum-file FILE] [--no-checksum] [--installers-dir DIR] [--out FILE]",
    ]
    .join("\n")
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

#[derive(Debug, Default)]
struct ArchiveSmokeOptions {
    archive: Option<PathBuf>,
    checksum_file: Option<PathBuf>,
    no_checksum: bool,
    installers_dir: Option<PathBuf>,
    out: Option<PathBuf>,
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

impl ArchiveSmokeOptions {
    fn parse(args: &[OsString]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut index = 0;

        while index < args.len() {
            let arg = args[index]
                .to_str()
                .ok_or_else(|| "arguments must be valid UTF-8".to_string())?;
            match arg {
                "--archive" => {
                    index += 1;
                    options.archive = Some(PathBuf::from(value(args, index, "--archive")?));
                }
                "--checksum-file" => {
                    index += 1;
                    options.checksum_file =
                        Some(PathBuf::from(value(args, index, "--checksum-file")?));
                }
                "--no-checksum" => options.no_checksum = true,
                "--installers-dir" => {
                    index += 1;
                    options.installers_dir =
                        Some(PathBuf::from(value(args, index, "--installers-dir")?));
                }
                "--out" => {
                    index += 1;
                    options.out = Some(PathBuf::from(value(args, index, "--out")?));
                }
                "--help" | "-h" => return Err(usage()),
                other => return Err(format!("unknown archive-smoke option `{other}`")),
            }
            index += 1;
        }

        if options.archive.is_none() {
            return Err("--archive is required".to_string());
        }
        if options.no_checksum && options.checksum_file.is_some() {
            return Err("--no-checksum cannot be combined with --checksum-file".to_string());
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
    let mut manifest = ReleaseManifest {
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
        archive_smoke_test: None,
    };
    write_json(&manifest_path, &manifest)?;

    let mut checksum_inputs = vec![archive.clone(), manifest_path.clone()];
    checksum_inputs.extend(installers);
    if let Some(sbom_path) = &sbom {
        checksum_inputs.push(sbom_path.clone());
    }
    let checksums = out_dir.join("SHA256SUMS");
    write_checksums(&checksums, &checksum_inputs)?;

    if !options.skip_smoke && target == host {
        let archive_smoke = archive_smoke(ArchiveSmokeRequest {
            archive: archive.clone(),
            checksum_file: Some(checksums.clone()),
            verify_checksum: true,
            installers_dir: Some(out_dir.clone()),
        })?;
        manifest.archive_smoke_test = Some(archive_smoke);
        write_json(&manifest_path, &manifest)?;
        write_checksums(&checksums, &checksum_inputs)?;
    }

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

fn archive_smoke_command(options: ArchiveSmokeOptions) -> Result<(), String> {
    let archive = options
        .archive
        .expect("archive presence was validated by option parsing");
    let request = ArchiveSmokeRequest {
        archive,
        checksum_file: options.checksum_file,
        verify_checksum: !options.no_checksum,
        installers_dir: options.installers_dir,
    };
    let evidence = archive_smoke(request)?;
    if let Some(out) = options.out {
        write_json(&out, &evidence)?;
        println!("archive smoke evidence: {}", out.display());
    } else {
        let json = serde_json::to_string_pretty(&evidence)
            .map_err(|error| format!("serialize archive smoke evidence: {error}"))?;
        println!("{json}");
    }
    Ok(())
}

struct ArchiveSmokeRequest {
    archive: PathBuf,
    checksum_file: Option<PathBuf>,
    verify_checksum: bool,
    installers_dir: Option<PathBuf>,
}

fn archive_smoke(request: ArchiveSmokeRequest) -> Result<ArchiveSmokeTest, String> {
    let archive = canonical_file(&request.archive, "archive")?;
    let checksum_file = resolve_checksum_file(&archive, request.checksum_file)?;
    let checksum_verified = if request.verify_checksum {
        let Some(checksum_file) = &checksum_file else {
            return Err(format!(
                "checksum file was not found beside {}; pass --checksum-file or --no-checksum",
                archive.display()
            ));
        };
        verify_checksum_entry(&archive, checksum_file)?;
        true
    } else {
        false
    };

    let temp_root = create_temp_dir("fileferry-archive-smoke")?;
    let result = (|| {
        extract_archive(&archive, &temp_root)?;
        let binary = find_extracted_binary(&temp_root)?;
        let binary_smoke = smoke_test_binary(&binary)?;
        let installer_smoke_tests =
            smoke_test_installers(&archive, request.installers_dir.as_deref(), &temp_root)?;
        Ok(ArchiveSmokeTest {
            archive: file_name(&archive)?,
            checksum_file: checksum_file
                .as_ref()
                .map(|path| file_name(path))
                .transpose()?,
            checksum_verified,
            extracted_binary: binary.display().to_string(),
            binary_smoke,
            installer_smoke_tests,
        })
    })();
    let cleanup = fs::remove_dir_all(&temp_root).map_err(|error| {
        format!(
            "remove archive smoke temp dir {}: {error}",
            temp_root.display()
        )
    });
    match (result, cleanup) {
        (Ok(evidence), Ok(())) => Ok(evidence),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
    }
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    let canonical =
        fs::canonicalize(path).map_err(|error| format!("{label} {}: {error}", path.display()))?;
    if !canonical.is_file() {
        return Err(format!("{label} is not a file: {}", canonical.display()));
    }
    Ok(canonical)
}

fn resolve_checksum_file(
    archive: &Path,
    checksum_file: Option<PathBuf>,
) -> Result<Option<PathBuf>, String> {
    if let Some(path) = checksum_file {
        return canonical_file(&path, "checksum file").map(Some);
    }
    let adjacent = archive
        .parent()
        .ok_or_else(|| format!("archive has no parent directory: {}", archive.display()))?
        .join("SHA256SUMS");
    if adjacent.is_file() {
        canonical_file(&adjacent, "checksum file").map(Some)
    } else {
        Ok(None)
    }
}

fn verify_checksum_entry(archive: &Path, checksum_file: &Path) -> Result<(), String> {
    let archive_name = file_name(archive)?;
    let checksums = fs::read_to_string(checksum_file)
        .map_err(|error| format!("read checksum file {}: {error}", checksum_file.display()))?;
    let expected = checksums
        .lines()
        .filter_map(parse_checksum_line)
        .find_map(|(hash, name)| (name == archive_name).then_some(hash))
        .ok_or_else(|| {
            format!(
                "no checksum entry for {archive_name} in {}",
                checksum_file.display()
            )
        })?;
    let actual = sha256_file(archive)?;
    if actual != expected {
        return Err(format!("checksum mismatch for {archive_name}"));
    }
    Ok(())
}

fn parse_checksum_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    let mut parts = trimmed.split_whitespace();
    let hash = parts.next()?;
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let name = parts.next()?;
    Some((
        hash.to_ascii_lowercase(),
        name.trim_start_matches('*').to_string(),
    ))
}

fn extract_archive(archive: &Path, destination: &Path) -> Result<(), String> {
    let mut command = Command::new("tar");
    command.arg("-xzf").arg(archive).arg("-C").arg(destination);
    run_command(&mut command, "extract release archive")
}

fn find_extracted_binary(root: &Path) -> Result<PathBuf, String> {
    let wanted = if cfg!(windows) { "ferry.exe" } else { "ferry" };
    let fallback = if cfg!(windows) { "ferry" } else { "ferry.exe" };
    let mut stack = vec![root.to_path_buf()];
    let mut fallback_match = None;
    while let Some(path) = stack.pop() {
        let entries =
            fs::read_dir(&path).map_err(|error| format!("read {}: {error}", path.display()))?;
        for entry in entries {
            let entry = entry.map_err(|error| format!("read {} entry: {error}", path.display()))?;
            let entry_path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("read file type for {}: {error}", entry_path.display()))?;
            if file_type.is_dir() {
                stack.push(entry_path);
            } else if file_type.is_file() {
                let name = entry_path.file_name().and_then(|name| name.to_str());
                if name == Some(wanted) {
                    return Ok(entry_path);
                }
                if name == Some(fallback) {
                    fallback_match = Some(entry_path);
                }
            }
        }
    }
    fallback_match.ok_or_else(|| format!("archive did not contain {wanted}"))
}

fn smoke_test_installers(
    archive: &Path,
    installers_dir: Option<&Path>,
    temp_root: &Path,
) -> Result<Vec<InstallerSmokeTest>, String> {
    let Some(installers_dir) = installers_dir else {
        return Ok(Vec::new());
    };
    let mut tests = Vec::new();
    let shell_installer = installers_dir.join("install.sh");
    if !cfg!(windows) && shell_installer.is_file() && command_available("sh") {
        tests.push(smoke_test_installer(
            "install.sh",
            Command::new("sh")
                .arg(&shell_installer)
                .arg("--archive")
                .arg(archive)
                .arg("--install-dir")
                .arg(temp_root.join("install-sh")),
            &temp_root.join("install-sh"),
        )?);
    }

    let powershell_installer = installers_dir.join("install.ps1");
    if powershell_installer.is_file() && command_available("pwsh") {
        tests.push(smoke_test_installer(
            "install.ps1",
            Command::new("pwsh")
                .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-File"])
                .arg(&powershell_installer)
                .arg("-Archive")
                .arg(archive)
                .arg("-InstallDir")
                .arg(temp_root.join("install-ps1")),
            &temp_root.join("install-ps1"),
        )?);
    }
    Ok(tests)
}

fn smoke_test_installer(
    installer: &str,
    command: &mut Command,
    install_dir: &Path,
) -> Result<InstallerSmokeTest, String> {
    let command_display = format_command(command);
    run_command(command, &format!("smoke test {installer}"))?;
    let installed = installed_binary_path(install_dir)?;
    let binary_smoke = smoke_test_binary(&installed)?;
    Ok(InstallerSmokeTest {
        installer: installer.to_string(),
        command: command_display,
        installed_binary: installed.display().to_string(),
        binary_smoke,
    })
}

fn installed_binary_path(install_dir: &Path) -> Result<PathBuf, String> {
    let native = install_dir.join(if cfg!(windows) { "ferry.exe" } else { "ferry" });
    if native.is_file() {
        return Ok(native);
    }
    for fallback in ["ferry", "ferry.exe"] {
        let path = install_dir.join(fallback);
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(format!(
        "installer did not write a ferry binary under {}",
        install_dir.display()
    ))
}

fn command_available(program: &str) -> bool {
    Command::new(program)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn create_temp_dir(prefix: &str) -> Result<PathBuf, String> {
    let root = env::temp_dir().join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("read system time: {error}"))?
            .as_nanos()
    ));
    fs::create_dir_all(&root)
        .map_err(|error| format!("create temp dir {}: {error}", root.display()))?;
    Ok(root)
}

fn format_command(command: &Command) -> String {
    let mut parts = Vec::new();
    parts.push(command.get_program().to_string_lossy().into_owned());
    parts.extend(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned()),
    );
    parts.join(" ")
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
    archive_smoke_test: Option<ArchiveSmokeTest>,
}

#[derive(Serialize)]
struct SmokeTest {
    command: String,
    exit_code: i32,
    stdout: String,
}

#[derive(Serialize)]
struct ArchiveSmokeTest {
    archive: String,
    checksum_file: Option<String>,
    checksum_verified: bool,
    extracted_binary: String,
    binary_smoke: SmokeTest,
    installer_smoke_tests: Vec<InstallerSmokeTest>,
}

#[derive(Serialize)]
struct InstallerSmokeTest {
    installer: String,
    command: String,
    installed_binary: String,
    binary_smoke: SmokeTest,
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
    fn parses_archive_smoke_options() {
        let args = [
            "--archive",
            "target/release-artifacts/fileferry-0.0.0-test.tar.gz",
            "--checksum-file",
            "target/release-artifacts/SHA256SUMS",
            "--installers-dir",
            "target/release-artifacts",
            "--out",
            "target/release-artifacts/archive-smoke.json",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();

        let options = ArchiveSmokeOptions::parse(&args).expect("options should parse");

        assert_eq!(
            options.archive.as_deref(),
            Some(Path::new(
                "target/release-artifacts/fileferry-0.0.0-test.tar.gz"
            ))
        );
        assert_eq!(
            options.checksum_file.as_deref(),
            Some(Path::new("target/release-artifacts/SHA256SUMS"))
        );
        assert_eq!(
            options.installers_dir.as_deref(),
            Some(Path::new("target/release-artifacts"))
        );
        assert_eq!(
            options.out.as_deref(),
            Some(Path::new("target/release-artifacts/archive-smoke.json"))
        );
    }

    #[test]
    fn rejects_archive_smoke_checksum_conflict() {
        let args = [
            "--archive",
            "fileferry.tar.gz",
            "--checksum-file",
            "SHA256SUMS",
            "--no-checksum",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();

        let error = ArchiveSmokeOptions::parse(&args).expect_err("combination should fail");

        assert!(error.contains("--no-checksum cannot be combined"));
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

    #[test]
    #[cfg(unix)]
    fn archive_smoke_extracts_archive_and_runs_installer_when_available() {
        let root = create_temp_dir("fileferry-xtask-archive-smoke-test").expect("temp root");
        let archive = create_test_archive(&root);
        let checksum = sha256_file(&archive).expect("hash archive");
        fs::write(
            root.join("SHA256SUMS"),
            format!(
                "{}  {}\n",
                checksum,
                archive
                    .file_name()
                    .and_then(|name| name.to_str())
                    .expect("archive file name")
            ),
        )
        .expect("write checksum file");
        let installers_dir = root.join("installers");
        fs::create_dir_all(&installers_dir).expect("create installers dir");
        fs::copy(
            workspace_root().join("scripts/install.sh"),
            installers_dir.join("install.sh"),
        )
        .expect("copy install.sh");

        let evidence = archive_smoke(ArchiveSmokeRequest {
            archive,
            checksum_file: None,
            verify_checksum: true,
            installers_dir: Some(installers_dir),
        })
        .expect("archive smoke should pass");

        fs::remove_dir_all(&root).expect("remove fixture root");

        assert!(evidence.checksum_verified);
        assert!(evidence.binary_smoke.stdout.contains("\"version\""));
        assert!(
            evidence
                .installer_smoke_tests
                .iter()
                .any(|test| test.installer == "install.sh"),
            "install.sh smoke path should run"
        );
    }

    #[cfg(unix)]
    fn create_test_archive(root: &Path) -> PathBuf {
        let package_dir = root.join("stage/fileferry-0.0.0-test-target");
        fs::create_dir_all(&package_dir).expect("create package dir");
        let binary = package_dir.join("ferry");
        fs::write(
            &binary,
            b"#!/bin/sh\nprintf '{\"version\":\"0.0.0-test\"}\\n'\n",
        )
        .expect("write fake ferry");

        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&binary).expect("stat binary").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&binary, permissions).expect("chmod binary");

        let archive = root.join("fileferry-0.0.0-test-target.tar.gz");
        let mut command = Command::new("tar");
        command
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(root.join("stage"))
            .arg("fileferry-0.0.0-test-target");
        run_command(&mut command, "create test archive").expect("create test archive");
        archive
    }

    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask lives under workspace root")
            .to_path_buf()
    }
}
