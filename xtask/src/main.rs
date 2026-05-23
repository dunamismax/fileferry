use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::{ErrorKind, Read};
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
        "verify-release-artifacts" => {
            verify_release_artifacts(VerifyReleaseArtifactsOptions::parse(&args[1..])?)
        }
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
        "  cargo run -p xtask -- archive-smoke --archive FILE [--target TRIPLE] [--checksum-file FILE] [--no-checksum] [--installers-dir DIR] [--expect-auditable] [--out FILE]",
        "  cargo run -p xtask -- verify-release-artifacts [--dir DIR] [--target TRIPLE] [--expect-signature]",
    ]
    .join("\n")
}

const FERRY_ROOT_PACKAGE: &str = "fileferry-cli";
const SIGSTORE_BUNDLE_NAME: &str = "SHA256SUMS.sigstore.json";
const RELEASE_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

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
    target: Option<String>,
    checksum_file: Option<PathBuf>,
    no_checksum: bool,
    installers_dir: Option<PathBuf>,
    expect_auditable: bool,
    out: Option<PathBuf>,
}

#[derive(Debug, Default)]
struct VerifyReleaseArtifactsOptions {
    dir: Option<PathBuf>,
    target: Option<String>,
    expect_signature: bool,
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
                "--target" => {
                    index += 1;
                    options.target = Some(value(args, index, "--target")?);
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
                "--expect-auditable" => options.expect_auditable = true,
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

impl VerifyReleaseArtifactsOptions {
    fn parse(args: &[OsString]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut index = 0;

        while index < args.len() {
            let arg = args[index]
                .to_str()
                .ok_or_else(|| "arguments must be valid UTF-8".to_string())?;
            match arg {
                "--dir" => {
                    index += 1;
                    options.dir = Some(PathBuf::from(value(args, index, "--dir")?));
                }
                "--target" => {
                    index += 1;
                    options.target = Some(value(args, index, "--target")?);
                }
                "--expect-signature" => options.expect_signature = true,
                "--help" | "-h" => return Err(usage()),
                other => return Err(format!("unknown verify-release-artifacts option `{other}`")),
            }
            index += 1;
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
    validate_release_target(&target)?;
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
    let auditable_metadata = if options.auditable {
        Some(verify_auditable_metadata(
            &binary,
            &target,
            "release binary",
        )?)
    } else {
        None
    };

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
        auditable_metadata,
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
            target: Some(target.clone()),
            checksum_file: Some(checksums.clone()),
            verify_checksum: true,
            installers_dir: Some(out_dir.clone()),
            expect_auditable: options.auditable,
        })?;
        manifest.archive_smoke_test = Some(archive_smoke);
        write_json(&manifest_path, &manifest)?;
        write_checksums(&checksums, &checksum_inputs)?;
    }

    let signature_bundle_path = out_dir.join(SIGSTORE_BUNDLE_NAME);
    let signature_bundle = if options.sign {
        let bundle = signature_bundle_path;
        sign_checksums(&checksums, &bundle)?;
        Some(bundle)
    } else {
        if let Some(removed) = remove_stale_sigstore_bundle(&out_dir)? {
            println!(
                "removed stale signature bundle for unsigned package: {}",
                removed.display()
            );
        }
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
    } else {
        println!("signature: unsigned package; {SIGSTORE_BUNDLE_NAME} was not produced");
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
        target: options.target,
        checksum_file: options.checksum_file,
        verify_checksum: !options.no_checksum,
        installers_dir: options.installers_dir,
        expect_auditable: options.expect_auditable,
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

fn verify_release_artifacts(options: VerifyReleaseArtifactsOptions) -> Result<(), String> {
    let workspace = env::current_dir().map_err(|error| format!("read current dir: {error}"))?;
    let dir = options
        .dir
        .unwrap_or_else(|| workspace.join("target").join("release-artifacts"));
    if !dir.is_dir() {
        return Err(format!(
            "artifact directory is not a directory: {}",
            dir.display()
        ));
    }
    let target = options.target.unwrap_or(host_triple()?);
    validate_release_target(&target)?;

    let archive = find_one_artifact(&dir, &target, "target archive", |name| {
        name.starts_with("fileferry-") && name.ends_with(&format!("-{target}.tar.gz"))
    })?;
    let manifest_path = find_one_artifact(&dir, &target, "release manifest JSON", |name| {
        name.starts_with("fileferry-") && name.ends_with(&format!("-{target}.manifest.json"))
    })?;
    let sbom_path = find_one_artifact(&dir, &target, "CycloneDX SBOM", |name| {
        name.starts_with("fileferry-") && name.ends_with(&format!("-{target}.cdx.json"))
    })?;
    let checksums_path = require_file(&dir.join("SHA256SUMS"), "checksum manifest")?;
    let install_sh = require_file(&dir.join("install.sh"), "Unix installer")?;
    let install_ps1 = require_file(&dir.join("install.ps1"), "PowerShell installer")?;

    let signature_path = dir.join(SIGSTORE_BUNDLE_NAME);
    if options.expect_signature {
        verify_sigstore_bundle(&signature_path)?;
    } else if signature_path.exists() {
        return Err(format!(
            "unexpected Sigstore bundle in unsigned artifact verification: {}; rerun with --expect-signature for signed artifacts or remove stale signing evidence",
            signature_path.display()
        ));
    }

    let archive_name = file_name(&archive)?;
    let archive_target = infer_release_target_from_archive_name(&archive)?;
    if archive_target != target {
        return Err(format!(
            "target archive `{archive_name}` encodes target `{archive_target}`, expected `{target}`"
        ));
    }
    let sbom_name = file_name(&sbom_path)?;
    let manifest_target = infer_release_target_from_artifact_name(
        &manifest_path,
        ".manifest.json",
        "release manifest JSON",
    )?;
    if manifest_target != target {
        return Err(format!(
            "release manifest JSON {} encodes target `{manifest_target}`, expected `{target}`",
            manifest_path.display()
        ));
    }
    let sbom_target =
        infer_release_target_from_artifact_name(&sbom_path, ".cdx.json", "CycloneDX SBOM")?;
    if sbom_target != target {
        return Err(format!(
            "CycloneDX SBOM {} encodes target `{sbom_target}`, expected `{target}`",
            sbom_path.display()
        ));
    }
    let manifest: ReleaseManifestEvidence = read_json_file(&manifest_path, "release manifest")?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "release manifest {} has unsupported schema_version {}",
            manifest_path.display(),
            manifest.schema_version
        ));
    }
    if manifest.package != "fileferry" || manifest.binary != "ferry" {
        return Err(format!(
            "release manifest {} is for package `{}` binary `{}`, expected fileferry/ferry",
            manifest_path.display(),
            manifest.package,
            manifest.binary
        ));
    }
    if manifest.target != target {
        return Err(format!(
            "release manifest {} targets `{}`, expected `{target}`",
            manifest_path.display(),
            manifest.target
        ));
    }
    if manifest.archive != archive_name {
        return Err(format!(
            "release manifest {} references archive `{}`, expected `{archive_name}`",
            manifest_path.display(),
            manifest.archive
        ));
    }
    if manifest.sbom.as_deref() != Some(sbom_name.as_str()) {
        return Err(format!(
            "release manifest {} references SBOM {:?}, expected `{sbom_name}`",
            manifest_path.display(),
            manifest.sbom
        ));
    }
    if !manifest.auditable_build {
        return Err(format!(
            "release manifest {} does not record an auditable build",
            manifest_path.display()
        ));
    }
    let Some(auditable_metadata) = &manifest.auditable_metadata else {
        return Err(format!(
            "release manifest {} does not include auditable metadata proof for target `{target}`",
            manifest_path.display()
        ));
    };
    verify_auditable_metadata_evidence(
        auditable_metadata,
        &target,
        &manifest.version,
        "release manifest auditable metadata",
    )?;
    require_manifest_installer(&manifest, "install.sh")?;
    require_manifest_installer(&manifest, "install.ps1")?;
    if manifest.commit.trim().is_empty() {
        return Err(format!(
            "release manifest {} has an empty commit",
            manifest_path.display()
        ));
    }
    if manifest.version.trim().is_empty() {
        return Err(format!(
            "release manifest {} has an empty version",
            manifest_path.display()
        ));
    }
    if let Some(smoke) = &manifest.smoke_test {
        verify_smoke_test(smoke, "release manifest host smoke")?;
    }
    if let Some(smoke) = &manifest.archive_smoke_test {
        verify_archive_smoke_test(
            smoke,
            &target,
            &archive_name,
            &manifest.version,
            "release manifest archive smoke",
        )?;
    }

    let sbom: serde_json::Value = read_json_file(&sbom_path, "CycloneDX SBOM")?;
    let bom_format = sbom
        .get("bomFormat")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "CycloneDX SBOM {} is missing bomFormat",
                sbom_path.display()
            )
        })?;
    if bom_format != "CycloneDX" {
        return Err(format!(
            "CycloneDX SBOM {} has bomFormat `{bom_format}`, expected `CycloneDX`",
            sbom_path.display()
        ));
    }

    let archive_smoke = find_archive_smoke_for_target(&dir, &target, &archive_name)?;
    verify_archive_smoke_test(
        &archive_smoke.evidence,
        &target,
        &archive_name,
        &manifest.version,
        "archive-smoke evidence JSON",
    )?;

    let checksum_entries = read_checksum_entries(&checksums_path)?;
    for path in [
        archive.as_path(),
        manifest_path.as_path(),
        sbom_path.as_path(),
        install_sh.as_path(),
        install_ps1.as_path(),
    ] {
        verify_checksum_for_file(path, &checksums_path, &checksum_entries)?;
    }

    println!(
        "verified release artifact evidence for {target} in {}",
        dir.display()
    );
    Ok(())
}

fn find_one_artifact(
    dir: &Path,
    target: &str,
    label: &str,
    matches_name: impl Fn(&str) -> bool,
) -> Result<PathBuf, String> {
    let mut matches = Vec::new();
    for entry in fs::read_dir(dir)
        .map_err(|error| format!("read artifact dir {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| format!("read artifact dir entry: {error}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if matches_name(name) {
            matches.push(path);
        }
    }
    match matches.len() {
        0 => Err(format!(
            "missing {label} for target `{target}` in {}",
            dir.display()
        )),
        1 => Ok(matches.remove(0)),
        _ => {
            matches.sort();
            let names = matches
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "expected one {label} for target `{target}`, found {}: {names}",
                matches.len()
            ))
        }
    }
}

fn require_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    if path.is_file() {
        Ok(path.to_path_buf())
    } else {
        Err(format!("missing {label}: {}", path.display()))
    }
}

fn verify_sigstore_bundle(path: &Path) -> Result<(), String> {
    let signature = require_file(path, "Sigstore bundle")?;
    let value: serde_json::Value = read_json_file(&signature, "Sigstore bundle")?;
    if !value.is_object() {
        return Err(format!(
            "Sigstore bundle {} is not a JSON object",
            signature.display()
        ));
    }
    Ok(())
}

fn remove_stale_sigstore_bundle(out_dir: &Path) -> Result<Option<PathBuf>, String> {
    let path = out_dir.join(SIGSTORE_BUNDLE_NAME);
    match fs::remove_file(&path) {
        Ok(()) => Ok(Some(path)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "remove stale Sigstore bundle {}: {error}",
            path.display()
        )),
    }
}

fn read_json_file<T: for<'de> Deserialize<'de>>(path: &Path, label: &str) -> Result<T, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("read {label} {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("parse {label} {}: {error}", path.display()))
}

fn require_manifest_installer(
    manifest: &ReleaseManifestEvidence,
    installer: &str,
) -> Result<(), String> {
    if manifest
        .installers
        .iter()
        .any(|candidate| candidate == installer)
    {
        Ok(())
    } else {
        Err(format!(
            "release manifest for target `{}` does not reference required installer `{installer}`",
            manifest.target
        ))
    }
}

struct ArchiveSmokeEvidenceFile {
    path: PathBuf,
    evidence: ArchiveSmokeTest,
}

fn find_archive_smoke_for_target(
    dir: &Path,
    target: &str,
    archive_name: &str,
) -> Result<ArchiveSmokeEvidenceFile, String> {
    let mut parsed = Vec::new();
    for entry in fs::read_dir(dir)
        .map_err(|error| format!("read artifact dir {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| format!("read artifact dir entry: {error}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".archive-smoke.json") {
            continue;
        }
        let evidence: ArchiveSmokeTest = read_json_file(&path, "archive-smoke evidence")?;
        parsed.push(ArchiveSmokeEvidenceFile { path, evidence });
    }

    let matches = parsed
        .into_iter()
        .filter(|smoke| smoke.evidence.archive == archive_name)
        .collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!(
            "missing archive-smoke JSON for target `{target}` archive `{archive_name}`"
        )),
        1 => {
            let mut matches = matches;
            let smoke = matches.remove(0);
            let smoke_name = file_name(&smoke.path)?;
            let expected_suffix = format!("{target}.archive-smoke.json");
            if !smoke_name.ends_with(&expected_suffix) {
                return Err(format!(
                    "archive-smoke JSON `{smoke_name}` references archive `{archive_name}` but does not encode target `{target}`"
                ));
            }
            Ok(smoke)
        }
        _ => {
            let mut matches = matches;
            matches.sort_by(|left, right| left.path.cmp(&right.path));
            let names = matches
                .iter()
                .map(|smoke| smoke.path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "expected one archive-smoke JSON for target `{target}` archive `{archive_name}`, found {}: {names}",
                matches.len()
            ))
        }
    }
}

fn verify_archive_smoke_test(
    smoke: &ArchiveSmokeTest,
    target: &str,
    archive_name: &str,
    version: &str,
    label: &str,
) -> Result<(), String> {
    if smoke.target != target {
        return Err(format!(
            "{label} targets `{}`, expected `{target}`",
            smoke.target
        ));
    }
    if smoke.archive != archive_name {
        return Err(format!(
            "{label} references archive `{}`, expected `{archive_name}`",
            smoke.archive
        ));
    }
    if smoke.checksum_file.as_deref() != Some("SHA256SUMS") {
        return Err(format!(
            "{label} references checksum file {:?}, expected `SHA256SUMS`",
            smoke.checksum_file
        ));
    }
    if !smoke.checksum_verified {
        return Err(format!("{label} did not verify SHA256SUMS"));
    }
    let Some(auditable_metadata) = &smoke.auditable_metadata else {
        return Err(format!(
            "{label} does not include packaged-binary auditable metadata proof for target `{target}`"
        ));
    };
    verify_auditable_metadata_evidence(
        auditable_metadata,
        target,
        version,
        &format!("{label} auditable metadata"),
    )?;
    verify_smoke_test(&smoke.binary_smoke, label)?;
    if smoke.installer_smoke_tests.is_empty() {
        return Err(format!("{label} contains no installer smoke tests"));
    }
    for installer in &smoke.installer_smoke_tests {
        if installer.installer != "install.sh" && installer.installer != "install.ps1" {
            return Err(format!(
                "{label} contains unexpected installer smoke test `{}`",
                installer.installer
            ));
        }
        verify_smoke_test(
            &installer.binary_smoke,
            &format!("{label} installer {}", installer.installer),
        )?;
    }
    Ok(())
}

fn verify_smoke_test(smoke: &SmokeTest, label: &str) -> Result<(), String> {
    if smoke.exit_code != 0 {
        return Err(format!(
            "{label} exited with {}, expected 0",
            smoke.exit_code
        ));
    }
    let stdout: serde_json::Value = serde_json::from_str(&smoke.stdout)
        .map_err(|error| format!("{label} stdout is not JSON: {error}"))?;
    if stdout_version(&stdout).is_none() {
        return Err(format!("{label} stdout JSON is missing version data"));
    }
    Ok(())
}

fn stdout_version(stdout: &serde_json::Value) -> Option<&str> {
    stdout
        .get("data")
        .and_then(|data| data.get("version"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| stdout.get("version").and_then(serde_json::Value::as_str))
}

fn verify_auditable_metadata(
    binary: &Path,
    target: &str,
    label: &str,
) -> Result<AuditableMetadataEvidence, String> {
    let info =
        auditable_info::audit_info_from_file(binary, Default::default()).map_err(|error| {
            format!(
                "{label} {} for target `{target}` does not contain readable cargo-auditable metadata: {error}",
                binary.display()
            )
        })?;
    let root_packages = info
        .packages
        .iter()
        .filter(|package| package.root)
        .collect::<Vec<_>>();
    if root_packages.len() != 1 {
        return Err(format!(
            "{label} {} for target `{target}` has {} cargo-auditable root packages, expected 1",
            binary.display(),
            root_packages.len()
        ));
    }
    let root = root_packages[0];
    if root.name != FERRY_ROOT_PACKAGE {
        return Err(format!(
            "{label} {} for target `{target}` has cargo-auditable root package `{}`, expected `{FERRY_ROOT_PACKAGE}`",
            binary.display(),
            root.name
        ));
    }
    if info.packages.is_empty() {
        return Err(format!(
            "{label} {} for target `{target}` has empty cargo-auditable package metadata",
            binary.display()
        ));
    }
    Ok(AuditableMetadataEvidence {
        target: target.to_string(),
        binary: file_name(binary)?,
        root_package: root.name.clone(),
        root_version: root.version.to_string(),
        package_count: info.packages.len(),
        format: info.format,
    })
}

fn verify_auditable_metadata_evidence(
    evidence: &AuditableMetadataEvidence,
    target: &str,
    version: &str,
    label: &str,
) -> Result<(), String> {
    if evidence.target != target {
        return Err(format!(
            "{label} targets `{}`, expected `{target}`",
            evidence.target
        ));
    }
    let expected_binary = if target.contains("windows") {
        "ferry.exe"
    } else {
        "ferry"
    };
    if evidence.binary != expected_binary {
        return Err(format!(
            "{label} references binary `{}`, expected `{expected_binary}` for target `{target}`",
            evidence.binary
        ));
    }
    if evidence.root_package != FERRY_ROOT_PACKAGE {
        return Err(format!(
            "{label} root package is `{}`, expected `{FERRY_ROOT_PACKAGE}`",
            evidence.root_package
        ));
    }
    if evidence.root_version != version {
        return Err(format!(
            "{label} root version is `{}`, expected manifest version `{version}`",
            evidence.root_version
        ));
    }
    if evidence.package_count == 0 {
        return Err(format!("{label} reports zero packages"));
    }
    Ok(())
}

fn read_checksum_entries(path: &Path) -> Result<Vec<(String, String)>, String> {
    let checksums = fs::read_to_string(path)
        .map_err(|error| format!("read checksum file {}: {error}", path.display()))?;
    let mut entries = Vec::new();
    for line in checksums.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let Some(entry) = parse_checksum_line(line) else {
            return Err(format!(
                "invalid checksum line in {}: {line:?}",
                path.display()
            ));
        };
        entries.push(entry);
    }
    Ok(entries)
}

fn verify_checksum_for_file(
    path: &Path,
    checksums_path: &Path,
    entries: &[(String, String)],
) -> Result<(), String> {
    let name = file_name(path)?;
    let expected = entries
        .iter()
        .find_map(|(hash, candidate)| (candidate == &name).then_some(hash))
        .ok_or_else(|| {
            format!(
                "missing checksum entry for `{name}` in {}",
                checksums_path.display()
            )
        })?;
    let actual = sha256_file(path)?;
    if &actual != expected {
        return Err(format!(
            "checksum mismatch for `{name}` in {}",
            checksums_path.display()
        ));
    }
    Ok(())
}

struct ArchiveSmokeRequest {
    archive: PathBuf,
    target: Option<String>,
    checksum_file: Option<PathBuf>,
    verify_checksum: bool,
    installers_dir: Option<PathBuf>,
    expect_auditable: bool,
}

fn archive_smoke(request: ArchiveSmokeRequest) -> Result<ArchiveSmokeTest, String> {
    let archive = canonical_file(&request.archive, "archive")?;
    let archive_target = infer_release_target_from_archive_name(&archive)?;
    let target = match request.target {
        Some(target) => {
            validate_release_target(&target)?;
            if target != archive_target {
                return Err(format!(
                    "archive {} encodes target `{archive_target}`, but --target was `{target}`",
                    file_name(&archive)?
                ));
            }
            target
        }
        None => archive_target,
    };
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
        let auditable_metadata =
            match verify_auditable_metadata(&binary, &target, "packaged archive binary") {
                Ok(evidence) => Some(evidence),
                Err(error) if request.expect_auditable => return Err(error),
                Err(_) => None,
            };
        let installer_smoke_tests =
            smoke_test_installers(&archive, request.installers_dir.as_deref(), &temp_root)?;
        Ok(ArchiveSmokeTest {
            target,
            archive: file_name(&archive)?,
            checksum_file: checksum_file
                .as_ref()
                .map(|path| file_name(path))
                .transpose()?,
            checksum_verified,
            extracted_binary: binary.display().to_string(),
            auditable_metadata,
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

fn infer_release_target_from_archive_name(archive: &Path) -> Result<String, String> {
    infer_release_target_from_artifact_name(archive, ".tar.gz", "archive")
}

fn infer_release_target_from_artifact_name(
    artifact: &Path,
    suffix: &str,
    label: &str,
) -> Result<String, String> {
    let artifact_name = file_name(artifact)?;
    let matches = RELEASE_TARGETS
        .iter()
        .copied()
        .filter(|target| artifact_name.ends_with(&format!("-{target}{suffix}")))
        .collect::<Vec<_>>();
    match matches.len() {
        1 => Ok(matches[0].to_string()),
        0 => Err(format!(
            "could not infer release target from {label} `{artifact_name}`"
        )),
        _ => Err(format!(
            "{label} `{artifact_name}` matched multiple release targets"
        )),
    }
}

fn validate_release_target(target: &str) -> Result<(), String> {
    if RELEASE_TARGETS.contains(&target) {
        Ok(())
    } else {
        Err(format!(
            "unsupported release target `{target}`; expected one of {}",
            RELEASE_TARGETS.join(", ")
        ))
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

#[derive(Debug, Deserialize)]
struct ReleaseManifestEvidence {
    schema_version: u8,
    package: String,
    binary: String,
    version: String,
    target: String,
    commit: String,
    archive: String,
    installers: Vec<String>,
    auditable_build: bool,
    auditable_metadata: Option<AuditableMetadataEvidence>,
    sbom: Option<String>,
    smoke_test: Option<SmokeTest>,
    archive_smoke_test: Option<ArchiveSmokeTest>,
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
    auditable_metadata: Option<AuditableMetadataEvidence>,
    sbom: Option<String>,
    smoke_test: Option<SmokeTest>,
    archive_smoke_test: Option<ArchiveSmokeTest>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AuditableMetadataEvidence {
    target: String,
    binary: String,
    root_package: String,
    root_version: String,
    package_count: usize,
    format: u32,
}

#[derive(Debug, Deserialize, Serialize)]
struct SmokeTest {
    command: String,
    exit_code: i32,
    stdout: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ArchiveSmokeTest {
    target: String,
    archive: String,
    checksum_file: Option<String>,
    checksum_verified: bool,
    extracted_binary: String,
    auditable_metadata: Option<AuditableMetadataEvidence>,
    binary_smoke: SmokeTest,
    installer_smoke_tests: Vec<InstallerSmokeTest>,
}

#[derive(Debug, Deserialize, Serialize)]
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
            "--target",
            "x86_64-unknown-linux-gnu",
            "--checksum-file",
            "target/release-artifacts/SHA256SUMS",
            "--installers-dir",
            "target/release-artifacts",
            "--expect-auditable",
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
        assert_eq!(options.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
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
        assert!(options.expect_auditable);
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
    fn parses_verify_release_artifacts_options() {
        let args = [
            "--dir",
            "target/release-artifacts",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--expect-signature",
        ]
        .into_iter()
        .map(OsString::from)
        .collect::<Vec<_>>();

        let options = VerifyReleaseArtifactsOptions::parse(&args).expect("options should parse");

        assert_eq!(
            options.dir.as_deref(),
            Some(Path::new("target/release-artifacts"))
        );
        assert_eq!(options.target.as_deref(), Some("x86_64-unknown-linux-gnu"));
        assert!(options.expect_signature);
    }

    #[test]
    fn verify_release_artifacts_accepts_complete_target_evidence() {
        let root = create_temp_dir("fileferry-xtask-verify-artifacts").expect("temp root");
        let target = "x86_64-unknown-linux-gnu";
        let stem = format!("fileferry-0.0.0-{target}");
        let archive_name = format!("{stem}.tar.gz");
        let manifest_name = format!("{stem}.manifest.json");
        let sbom_name = format!("{stem}.cdx.json");
        let smoke_name = format!("fileferry-{target}.archive-smoke.json");
        fs::write(root.join(&archive_name), b"archive").expect("write archive");
        fs::write(
            root.join(&sbom_name),
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
        )
        .expect("write sbom");
        fs::write(root.join("install.sh"), b"#!/bin/sh\n").expect("write install.sh");
        fs::write(root.join("install.ps1"), b"Write-Output ferry\n").expect("write install.ps1");
        fs::write(root.join("SHA256SUMS.sigstore.json"), r#"{"bundle":true}"#)
            .expect("write signature bundle");

        let archive_smoke = test_archive_smoke(target, &archive_name);
        let manifest = ReleaseManifest {
            schema_version: 1,
            package: "fileferry",
            binary: "ferry",
            version: "0.0.0",
            target,
            commit: "0123456789abcdef",
            archive: archive_name.clone(),
            installers: vec!["install.sh".to_string(), "install.ps1".to_string()],
            auditable_build: true,
            auditable_metadata: Some(test_auditable_metadata(target)),
            sbom: Some(sbom_name.clone()),
            smoke_test: Some(test_smoke()),
            archive_smoke_test: Some(test_archive_smoke(target, &archive_name)),
        };
        write_json(&root.join(&manifest_name), &manifest).expect("write manifest");
        write_json(&root.join(&smoke_name), &archive_smoke).expect("write archive smoke");
        write_checksums(
            &root.join("SHA256SUMS"),
            &[
                root.join(&archive_name),
                root.join(&manifest_name),
                root.join(&sbom_name),
                root.join("install.sh"),
                root.join("install.ps1"),
            ],
        )
        .expect("write checksums");

        verify_release_artifacts(VerifyReleaseArtifactsOptions {
            dir: Some(root.clone()),
            target: Some(target.to_string()),
            expect_signature: true,
        })
        .expect("complete evidence should verify");

        fs::remove_dir_all(&root).expect("remove fixture root");
    }

    #[test]
    fn verify_release_artifacts_rejects_wrong_manifest_target() {
        let root = create_temp_dir("fileferry-xtask-verify-target").expect("temp root");
        let target = "x86_64-unknown-linux-gnu";
        let wrong_target = "aarch64-unknown-linux-gnu";
        let stem = format!("fileferry-0.0.0-{target}");
        let archive_name = format!("{stem}.tar.gz");
        let manifest_name = format!("{stem}.manifest.json");
        let sbom_name = format!("{stem}.cdx.json");
        fs::write(root.join(&archive_name), b"archive").expect("write archive");
        fs::write(
            root.join(&sbom_name),
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
        )
        .expect("write sbom");
        fs::write(root.join("install.sh"), b"#!/bin/sh\n").expect("write install.sh");
        fs::write(root.join("install.ps1"), b"Write-Output ferry\n").expect("write install.ps1");
        let manifest = ReleaseManifest {
            schema_version: 1,
            package: "fileferry",
            binary: "ferry",
            version: "0.0.0",
            target: wrong_target,
            commit: "0123456789abcdef",
            archive: archive_name.clone(),
            installers: vec!["install.sh".to_string(), "install.ps1".to_string()],
            auditable_build: true,
            auditable_metadata: Some(test_auditable_metadata(wrong_target)),
            sbom: Some(sbom_name.clone()),
            smoke_test: None,
            archive_smoke_test: None,
        };
        write_json(&root.join(&manifest_name), &manifest).expect("write manifest");
        write_json(
            &root.join(format!("fileferry-{target}.archive-smoke.json")),
            &test_archive_smoke(target, &archive_name),
        )
        .expect("write archive smoke");
        write_checksums(
            &root.join("SHA256SUMS"),
            &[
                root.join(&archive_name),
                root.join(&manifest_name),
                root.join(&sbom_name),
                root.join("install.sh"),
                root.join("install.ps1"),
            ],
        )
        .expect("write checksums");

        let error = verify_release_artifacts(VerifyReleaseArtifactsOptions {
            dir: Some(root.clone()),
            target: Some(target.to_string()),
            expect_signature: false,
        })
        .expect_err("wrong target should fail");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert!(error.contains("targets `aarch64-unknown-linux-gnu`"));
    }

    #[test]
    fn verify_release_artifacts_requires_signature_when_expected() {
        let root = create_temp_dir("fileferry-xtask-verify-signature").expect("temp root");
        let target = "x86_64-unknown-linux-gnu";
        let stem = format!("fileferry-0.0.0-{target}");
        let archive_name = format!("{stem}.tar.gz");
        let manifest_name = format!("{stem}.manifest.json");
        let sbom_name = format!("{stem}.cdx.json");
        fs::write(root.join(&archive_name), b"archive").expect("write archive");
        fs::write(
            root.join(&sbom_name),
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
        )
        .expect("write sbom");
        fs::write(root.join("install.sh"), b"#!/bin/sh\n").expect("write install.sh");
        fs::write(root.join("install.ps1"), b"Write-Output ferry\n").expect("write install.ps1");
        let manifest = ReleaseManifest {
            schema_version: 1,
            package: "fileferry",
            binary: "ferry",
            version: "0.0.0",
            target,
            commit: "0123456789abcdef",
            archive: archive_name.clone(),
            installers: vec!["install.sh".to_string(), "install.ps1".to_string()],
            auditable_build: true,
            auditable_metadata: Some(test_auditable_metadata(target)),
            sbom: Some(sbom_name.clone()),
            smoke_test: None,
            archive_smoke_test: None,
        };
        write_json(&root.join(&manifest_name), &manifest).expect("write manifest");
        write_json(
            &root.join(format!("fileferry-{target}.archive-smoke.json")),
            &test_archive_smoke(target, &archive_name),
        )
        .expect("write archive smoke");
        write_checksums(
            &root.join("SHA256SUMS"),
            &[
                root.join(&archive_name),
                root.join(&manifest_name),
                root.join(&sbom_name),
                root.join("install.sh"),
                root.join("install.ps1"),
            ],
        )
        .expect("write checksums");

        let error = verify_release_artifacts(VerifyReleaseArtifactsOptions {
            dir: Some(root.clone()),
            target: Some(target.to_string()),
            expect_signature: true,
        })
        .expect_err("missing signature should fail");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert!(error.contains("missing Sigstore bundle"));
    }

    #[test]
    fn verify_release_artifacts_rejects_signature_when_not_expected() {
        let root =
            create_temp_dir("fileferry-xtask-verify-unexpected-signature").expect("temp root");
        let target = "x86_64-unknown-linux-gnu";
        write_verify_release_fixture(&root, target, Some(r#"{"bundle":true}"#));

        let error = verify_release_artifacts(VerifyReleaseArtifactsOptions {
            dir: Some(root.clone()),
            target: Some(target.to_string()),
            expect_signature: false,
        })
        .expect_err("unexpected signature should fail unsigned verification");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert!(error.contains("unexpected Sigstore bundle"));
        assert!(error.contains("--expect-signature"));
    }

    #[test]
    fn verify_release_artifacts_rejects_malformed_signature_when_expected() {
        let root = create_temp_dir("fileferry-xtask-verify-bad-signature").expect("temp root");
        let target = "x86_64-unknown-linux-gnu";
        write_verify_release_fixture(&root, target, Some("[]"));

        let error = verify_release_artifacts(VerifyReleaseArtifactsOptions {
            dir: Some(root.clone()),
            target: Some(target.to_string()),
            expect_signature: true,
        })
        .expect_err("malformed signature bundle should fail signed verification");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert!(error.contains("Sigstore bundle"));
        assert!(error.contains("is not a JSON object"));
    }

    #[test]
    fn remove_stale_sigstore_bundle_deletes_unsigned_package_leftover() {
        let root = create_temp_dir("fileferry-xtask-stale-signature").expect("temp root");
        let bundle = root.join(SIGSTORE_BUNDLE_NAME);
        fs::write(&bundle, r#"{"bundle":true}"#).expect("write stale bundle");

        let removed = remove_stale_sigstore_bundle(&root).expect("remove stale bundle");
        let second = remove_stale_sigstore_bundle(&root).expect("ignore missing stale bundle");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert_eq!(removed.as_deref(), Some(bundle.as_path()));
        assert!(second.is_none());
    }

    #[test]
    fn verify_release_artifacts_rejects_wrong_archive_smoke_filename_target() {
        let root = create_temp_dir("fileferry-xtask-verify-smoke-name").expect("temp root");
        let target = "x86_64-unknown-linux-gnu";
        let wrong_target = "aarch64-unknown-linux-gnu";
        let stem = format!("fileferry-0.0.0-{target}");
        let archive_name = format!("{stem}.tar.gz");
        let manifest_name = format!("{stem}.manifest.json");
        let sbom_name = format!("{stem}.cdx.json");
        fs::write(root.join(&archive_name), b"archive").expect("write archive");
        fs::write(
            root.join(&sbom_name),
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
        )
        .expect("write sbom");
        fs::write(root.join("install.sh"), b"#!/bin/sh\n").expect("write install.sh");
        fs::write(root.join("install.ps1"), b"Write-Output ferry\n").expect("write install.ps1");
        let manifest = ReleaseManifest {
            schema_version: 1,
            package: "fileferry",
            binary: "ferry",
            version: "0.0.0",
            target,
            commit: "0123456789abcdef",
            archive: archive_name.clone(),
            installers: vec!["install.sh".to_string(), "install.ps1".to_string()],
            auditable_build: true,
            auditable_metadata: Some(test_auditable_metadata(target)),
            sbom: Some(sbom_name.clone()),
            smoke_test: None,
            archive_smoke_test: None,
        };
        write_json(&root.join(&manifest_name), &manifest).expect("write manifest");
        write_json(
            &root.join(format!("fileferry-{wrong_target}.archive-smoke.json")),
            &test_archive_smoke(target, &archive_name),
        )
        .expect("write archive smoke");
        write_checksums(
            &root.join("SHA256SUMS"),
            &[
                root.join(&archive_name),
                root.join(&manifest_name),
                root.join(&sbom_name),
                root.join("install.sh"),
                root.join("install.ps1"),
            ],
        )
        .expect("write checksums");

        let error = verify_release_artifacts(VerifyReleaseArtifactsOptions {
            dir: Some(root.clone()),
            target: Some(target.to_string()),
            expect_signature: false,
        })
        .expect_err("wrong archive-smoke filename target should fail");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert!(error.contains("archive-smoke JSON"));
        assert!(error.contains("does not encode target `x86_64-unknown-linux-gnu`"));
    }

    #[test]
    fn verify_archive_smoke_rejects_target_mismatch() {
        let target = "x86_64-unknown-linux-gnu";
        let archive_name = format!("fileferry-0.0.0-{target}.tar.gz");
        let mut smoke = test_archive_smoke("aarch64-unknown-linux-gnu", &archive_name);

        let error = verify_archive_smoke_test(
            &smoke,
            target,
            &archive_name,
            "0.0.0",
            "archive-smoke evidence JSON",
        )
        .expect_err("target mismatch should fail");

        assert!(error.contains("targets `aarch64-unknown-linux-gnu`"));

        smoke.target = target.to_string();
        smoke.auditable_metadata = Some(test_auditable_metadata(target));
        verify_archive_smoke_test(
            &smoke,
            target,
            &archive_name,
            "0.0.0",
            "archive-smoke evidence JSON",
        )
        .expect("corrected smoke evidence should verify");
    }

    #[test]
    fn verify_archive_smoke_requires_packaged_binary_auditable_metadata() {
        let target = "x86_64-unknown-linux-gnu";
        let archive_name = format!("fileferry-0.0.0-{target}.tar.gz");
        let mut smoke = test_archive_smoke(target, &archive_name);
        smoke.auditable_metadata = None;

        let error = verify_archive_smoke_test(
            &smoke,
            target,
            &archive_name,
            "0.0.0",
            "archive-smoke evidence JSON",
        )
        .expect_err("missing packaged-binary auditable proof should fail");

        assert!(error.contains("packaged-binary auditable metadata proof"));
    }

    #[test]
    fn archive_smoke_rejects_target_that_does_not_match_archive_filename() {
        let root = create_temp_dir("fileferry-xtask-archive-target").expect("temp root");
        let archive = root.join("fileferry-0.0.0-aarch64-unknown-linux-gnu.tar.gz");
        fs::write(&archive, b"not a tar archive").expect("write fake archive");

        let error = archive_smoke(ArchiveSmokeRequest {
            archive,
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            checksum_file: None,
            verify_checksum: false,
            installers_dir: None,
            expect_auditable: false,
        })
        .expect_err("archive filename target mismatch should fail");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert!(error.contains("encodes target `aarch64-unknown-linux-gnu`"));
        assert!(error.contains("--target was `x86_64-unknown-linux-gnu`"));
    }

    #[test]
    fn archive_smoke_rejects_archive_without_release_target_filename() {
        let root = create_temp_dir("fileferry-xtask-archive-no-target").expect("temp root");
        let archive = root.join("fileferry-0.0.0-test.tar.gz");
        fs::write(&archive, b"not a tar archive").expect("write fake archive");

        let error = archive_smoke(ArchiveSmokeRequest {
            archive,
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            checksum_file: None,
            verify_checksum: false,
            installers_dir: None,
            expect_auditable: false,
        })
        .expect_err("archive filename without release target should fail");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert!(error.contains("could not infer release target from archive"));
    }

    #[test]
    fn verify_release_artifacts_rejects_unsupported_target() {
        let root = create_temp_dir("fileferry-xtask-unsupported-target").expect("temp root");
        let error = verify_release_artifacts(VerifyReleaseArtifactsOptions {
            dir: Some(root.clone()),
            target: Some("x86_64-unknown-freebsd".to_string()),
            expect_signature: false,
        })
        .expect_err("unsupported release target should fail");

        fs::remove_dir_all(&root).expect("remove fixture root");
        assert!(error.contains("unsupported release target `x86_64-unknown-freebsd`"));
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
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            checksum_file: None,
            verify_checksum: true,
            installers_dir: Some(installers_dir),
            expect_auditable: false,
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
        let package_dir = root.join("stage/fileferry-0.0.0-x86_64-unknown-linux-gnu");
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

        let archive = root.join("fileferry-0.0.0-x86_64-unknown-linux-gnu.tar.gz");
        let mut command = Command::new("tar");
        command
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(root.join("stage"))
            .arg("fileferry-0.0.0-x86_64-unknown-linux-gnu");
        run_command(&mut command, "create test archive").expect("create test archive");
        archive
    }

    fn test_archive_smoke(target: &str, archive_name: &str) -> ArchiveSmokeTest {
        ArchiveSmokeTest {
            target: target.to_string(),
            archive: archive_name.to_string(),
            checksum_file: Some("SHA256SUMS".to_string()),
            checksum_verified: true,
            extracted_binary: "/tmp/fileferry/ferry".to_string(),
            auditable_metadata: Some(test_auditable_metadata(target)),
            binary_smoke: test_smoke(),
            installer_smoke_tests: vec![InstallerSmokeTest {
                installer: "install.sh".to_string(),
                command: "sh install.sh".to_string(),
                installed_binary: "/tmp/fileferry/bin/ferry".to_string(),
                binary_smoke: test_smoke(),
            }],
        }
    }

    fn write_verify_release_fixture(root: &Path, target: &str, signature_json: Option<&str>) {
        let stem = format!("fileferry-0.0.0-{target}");
        let archive_name = format!("{stem}.tar.gz");
        let manifest_name = format!("{stem}.manifest.json");
        let sbom_name = format!("{stem}.cdx.json");
        let smoke_name = format!("fileferry-{target}.archive-smoke.json");
        fs::write(root.join(&archive_name), b"archive").expect("write archive");
        fs::write(
            root.join(&sbom_name),
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
        )
        .expect("write sbom");
        fs::write(root.join("install.sh"), b"#!/bin/sh\n").expect("write install.sh");
        fs::write(root.join("install.ps1"), b"Write-Output ferry\n").expect("write install.ps1");
        if let Some(signature_json) = signature_json {
            fs::write(root.join(SIGSTORE_BUNDLE_NAME), signature_json)
                .expect("write signature bundle");
        }

        let manifest = ReleaseManifest {
            schema_version: 1,
            package: "fileferry",
            binary: "ferry",
            version: "0.0.0",
            target,
            commit: "0123456789abcdef",
            archive: archive_name.clone(),
            installers: vec!["install.sh".to_string(), "install.ps1".to_string()],
            auditable_build: true,
            auditable_metadata: Some(test_auditable_metadata(target)),
            sbom: Some(sbom_name.clone()),
            smoke_test: Some(test_smoke()),
            archive_smoke_test: Some(test_archive_smoke(target, &archive_name)),
        };
        write_json(&root.join(&manifest_name), &manifest).expect("write manifest");
        write_json(
            &root.join(&smoke_name),
            &test_archive_smoke(target, &archive_name),
        )
        .expect("write archive smoke");
        write_checksums(
            &root.join("SHA256SUMS"),
            &[
                root.join(&archive_name),
                root.join(&manifest_name),
                root.join(&sbom_name),
                root.join("install.sh"),
                root.join("install.ps1"),
            ],
        )
        .expect("write checksums");
    }

    fn test_auditable_metadata(target: &str) -> AuditableMetadataEvidence {
        AuditableMetadataEvidence {
            target: target.to_string(),
            binary: if target.contains("windows") {
                "ferry.exe".to_string()
            } else {
                "ferry".to_string()
            },
            root_package: FERRY_ROOT_PACKAGE.to_string(),
            root_version: "0.0.0".to_string(),
            package_count: 42,
            format: 0,
        }
    }

    fn test_smoke() -> SmokeTest {
        SmokeTest {
            command: "ferry version --json".to_string(),
            exit_code: 0,
            stdout: serde_json::json!({
                "schema_version": 1,
                "command": "version",
                "status": "success",
                "data": {
                    "command": "ferry",
                    "version": "0.0.0"
                }
            })
            .to_string(),
        }
    }

    #[cfg(unix)]
    fn workspace_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("xtask lives under workspace root")
            .to_path_buf()
    }
}
