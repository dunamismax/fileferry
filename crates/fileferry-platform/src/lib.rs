//! Cross-platform path and filesystem metadata behavior.

use serde::{Deserialize, Serialize};
use std::{
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("metadata for {path} could not be read")]
    MetadataRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("symlink target for {path} could not be read")]
    SymlinkTargetRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    RegularFile,
    Directory,
    Symlink,
    Other,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataValue<T> {
    Captured(T),
    #[default]
    Unsupported,
    Denied(String),
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Windows,
    Macos,
    Linux,
    Freebsd,
    Netbsd,
    Openbsd,
    Unix,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseBehavior {
    CaseSensitive,
    CaseInsensitive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathFacts {
    pub normalized_relative: bool,
    pub has_parent_component: bool,
    pub has_root_or_prefix: bool,
    pub has_windows_reserved_name: bool,
    pub segment_count: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct Timestamp {
    pub seconds: i64,
    pub nanoseconds: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct EntryMetadata {
    pub kind: EntryKind,
    #[serde(default)]
    pub source_platform: PlatformKind,
    pub size_bytes: Option<u64>,
    pub modified: MetadataValue<Timestamp>,
    pub created: MetadataValue<Timestamp>,
    pub symlink_target: MetadataValue<PathBuf>,
    pub unix: Option<UnixMetadata>,
    #[serde(default)]
    pub extensions: MetadataExtensions,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct UnixMetadata {
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct MetadataExtensions {
    #[serde(default)]
    pub xattrs: MetadataValue<MetadataFieldSummary>,
    #[serde(default)]
    pub acls: MetadataValue<MetadataFieldSummary>,
    #[serde(default)]
    pub file_flags: MetadataValue<MetadataFieldSummary>,
    #[serde(default)]
    pub windows_attributes: MetadataValue<MetadataFieldSummary>,
}

impl Default for MetadataExtensions {
    fn default() -> Self {
        Self {
            xattrs: MetadataValue::Unsupported,
            acls: MetadataValue::Unsupported,
            file_flags: MetadataValue::Unsupported,
            windows_attributes: MetadataValue::Unsupported,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct MetadataFieldSummary {
    pub count: usize,
}

pub fn capture_metadata(path: impl AsRef<Path>) -> Result<EntryMetadata, PlatformError> {
    let path = path.as_ref();
    let metadata = fs::symlink_metadata(path).map_err(|source| PlatformError::MetadataRead {
        path: path.to_path_buf(),
        source,
    })?;

    let file_type = metadata.file_type();
    let kind = if file_type.is_file() {
        EntryKind::RegularFile
    } else if file_type.is_dir() {
        EntryKind::Directory
    } else if file_type.is_symlink() {
        EntryKind::Symlink
    } else {
        EntryKind::Other
    };

    let symlink_target = if kind == EntryKind::Symlink {
        match fs::read_link(path) {
            Ok(target) => MetadataValue::Captured(target),
            Err(source) if source.kind() == io::ErrorKind::PermissionDenied => {
                MetadataValue::Denied(source.to_string())
            }
            Err(source) => {
                return Err(PlatformError::SymlinkTargetRead {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    } else {
        MetadataValue::Unsupported
    };

    Ok(EntryMetadata {
        kind,
        source_platform: current_platform(),
        size_bytes: if file_type.is_file() {
            Some(metadata.len())
        } else {
            None
        },
        modified: metadata_value_from_time(metadata.modified()),
        created: metadata_value_from_time(metadata.created()),
        symlink_target,
        unix: unix_metadata(&metadata),
        extensions: metadata_extensions(path),
    })
}

pub const fn current_platform() -> PlatformKind {
    if cfg!(windows) {
        PlatformKind::Windows
    } else if cfg!(target_os = "macos") {
        PlatformKind::Macos
    } else if cfg!(target_os = "linux") {
        PlatformKind::Linux
    } else if cfg!(target_os = "freebsd") {
        PlatformKind::Freebsd
    } else if cfg!(target_os = "netbsd") {
        PlatformKind::Netbsd
    } else if cfg!(target_os = "openbsd") {
        PlatformKind::Openbsd
    } else if cfg!(unix) {
        PlatformKind::Unix
    } else {
        PlatformKind::Unknown
    }
}

pub fn path_facts(path: impl AsRef<Path>) -> PathFacts {
    let mut normalized = PathBuf::new();
    let mut has_parent_component = false;
    let mut has_root_or_prefix = false;
    let mut has_windows_reserved_name = false;
    let mut segment_count = 0_usize;

    for component in path.as_ref().components() {
        match component {
            Component::Normal(segment) => {
                segment_count += 1;
                normalized.push(segment);
                has_windows_reserved_name |= is_windows_reserved_name(segment);
            }
            Component::CurDir => {}
            Component::ParentDir => has_parent_component = true,
            Component::RootDir | Component::Prefix(_) => has_root_or_prefix = true,
        }
    }

    PathFacts {
        normalized_relative: !has_parent_component
            && !has_root_or_prefix
            && normalized == path.as_ref(),
        has_parent_component,
        has_root_or_prefix,
        has_windows_reserved_name,
        segment_count,
    }
}

pub fn is_windows_reserved_name(segment: &OsStr) -> bool {
    let Some(name) = segment.to_str() else {
        return false;
    };
    let stem = name
        .trim_end_matches([' ', '.'])
        .split_once('.')
        .map_or(name.trim_end_matches([' ', '.']), |(stem, _)| stem);
    let upper = stem.to_ascii_uppercase();

    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || reserved_numbered_device(&upper, "COM")
        || reserved_numbered_device(&upper, "LPT")
}

fn reserved_numbered_device(upper: &str, prefix: &str) -> bool {
    let Some(number) = upper.strip_prefix(prefix) else {
        return false;
    };

    matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
}

pub fn probe_case_behavior(directory: impl AsRef<Path>) -> io::Result<CaseBehavior> {
    let directory = directory.as_ref();
    let now = Timestamp::from(SystemTime::now());
    let probe_name = format!(
        "fileferry-case-probe-{}-{}-{}",
        std::process::id(),
        now.seconds,
        now.nanoseconds
    );
    let lower = directory.join(&probe_name);
    let upper = directory.join(probe_name.to_ascii_uppercase());

    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lower)?;
    file.write_all(b"case")?;
    drop(file);
    let behavior = if upper.exists() {
        CaseBehavior::CaseInsensitive
    } else {
        CaseBehavior::CaseSensitive
    };
    fs::remove_file(&lower)?;

    Ok(behavior)
}

fn metadata_value_from_time(result: io::Result<SystemTime>) -> MetadataValue<Timestamp> {
    match result {
        Ok(time) => MetadataValue::Captured(Timestamp::from(time)),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            MetadataValue::Denied(error.to_string())
        }
        Err(_) => MetadataValue::Unsupported,
    }
}

fn metadata_extensions(path: &Path) -> MetadataExtensions {
    MetadataExtensions {
        xattrs: xattr_summary(path),
        acls: MetadataValue::Unsupported,
        file_flags: MetadataValue::Unsupported,
        windows_attributes: MetadataValue::Unsupported,
    }
}

fn xattr_summary(path: &Path) -> MetadataValue<MetadataFieldSummary> {
    if !xattr::SUPPORTED_PLATFORM {
        return MetadataValue::Unsupported;
    }

    match xattr::list(path) {
        Ok(names) => MetadataValue::Captured(MetadataFieldSummary {
            count: names.filter(|name| reportable_xattr_name(name)).count(),
        }),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            MetadataValue::Denied(error.to_string())
        }
        Err(_) => MetadataValue::Unsupported,
    }
}

fn reportable_xattr_name(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return true;
    };

    name != "com.apple.provenance"
}

impl From<SystemTime> for Timestamp {
    fn from(value: SystemTime) -> Self {
        match value.duration_since(UNIX_EPOCH) {
            Ok(duration) => Self {
                seconds: duration.as_secs() as i64,
                nanoseconds: duration.subsec_nanos(),
            },
            Err(error) => {
                let duration = error.duration();
                if duration.subsec_nanos() == 0 {
                    Self {
                        seconds: -(duration.as_secs() as i64),
                        nanoseconds: 0,
                    }
                } else {
                    Self {
                        seconds: -(duration.as_secs() as i64) - 1,
                        nanoseconds: 1_000_000_000 - duration.subsec_nanos(),
                    }
                }
            }
        }
    }
}

#[cfg(unix)]
fn unix_metadata(metadata: &fs::Metadata) -> Option<UnixMetadata> {
    use std::os::unix::fs::MetadataExt;

    Some(UnixMetadata {
        mode: metadata.mode(),
        uid: metadata.uid(),
        gid: metadata.gid(),
    })
}

#[cfg(not(unix))]
fn unix_metadata(_metadata: &fs::Metadata) -> Option<UnixMetadata> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_regular_file_metadata_without_reading_contents() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.txt");
        fs::write(&path, b"hello").expect("write file");

        let metadata = capture_metadata(&path).expect("metadata");

        assert_eq!(metadata.kind, EntryKind::RegularFile);
        assert_eq!(metadata.source_platform, current_platform());
        assert_eq!(metadata.size_bytes, Some(5));
        assert!(matches!(
            metadata.modified,
            MetadataValue::Captured(Timestamp { .. })
        ));
        assert!(matches!(
            metadata.symlink_target,
            MetadataValue::Unsupported
        ));
        assert!(matches!(
            metadata.extensions.xattrs,
            MetadataValue::Captured(MetadataFieldSummary { .. }) | MetadataValue::Unsupported
        ));
        assert_eq!(metadata.extensions.acls, MetadataValue::Unsupported);
        assert_eq!(metadata.extensions.file_flags, MetadataValue::Unsupported);
        assert_eq!(
            metadata.extensions.windows_attributes,
            MetadataValue::Unsupported
        );
    }

    #[test]
    fn captures_directory_metadata() {
        let temp = tempfile::tempdir().expect("tempdir");

        let metadata = capture_metadata(temp.path()).expect("metadata");

        assert_eq!(metadata.kind, EntryKind::Directory);
        assert_eq!(metadata.size_bytes, None);
    }

    #[test]
    fn deserializes_older_metadata_without_source_platform_as_unknown() {
        let metadata: EntryMetadata = serde_json::from_str(
            r#"{
                "kind": "regular_file",
                "size_bytes": 5,
                "modified": { "captured": { "seconds": 1, "nanoseconds": 0 } },
                "created": "unsupported",
                "symlink_target": "unsupported",
                "unix": null
            }"#,
        )
        .expect("deserialize metadata");

        assert_eq!(metadata.source_platform, PlatformKind::Unknown);
        assert_eq!(metadata.extensions, MetadataExtensions::default());
    }

    #[test]
    fn deserializes_xattr_only_extensions_with_default_extension_statuses() {
        let metadata: EntryMetadata = serde_json::from_str(
            r#"{
                "kind": "regular_file",
                "source_platform": "linux",
                "size_bytes": 5,
                "modified": { "captured": { "seconds": 1, "nanoseconds": 0 } },
                "created": "unsupported",
                "symlink_target": "unsupported",
                "unix": null,
                "extensions": {
                    "xattrs": { "captured": { "count": 2 } }
                }
            }"#,
        )
        .expect("deserialize metadata");

        assert_eq!(
            metadata.extensions.xattrs,
            MetadataValue::Captured(MetadataFieldSummary { count: 2 })
        );
        assert_eq!(metadata.extensions.acls, MetadataValue::Unsupported);
        assert_eq!(metadata.extensions.file_flags, MetadataValue::Unsupported);
        assert_eq!(
            metadata.extensions.windows_attributes,
            MetadataValue::Unsupported
        );
    }

    #[test]
    fn captures_xattr_presence_when_filesystem_exposes_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.txt");
        fs::write(&path, b"hello").expect("write file");
        if xattr::set(&path, test_xattr_name(), b"present").is_err() {
            return;
        }

        let metadata = capture_metadata(&path).expect("metadata");

        assert!(matches!(
            metadata.extensions.xattrs,
            MetadataValue::Captured(MetadataFieldSummary { count }) if count >= 1
        ));
    }

    #[test]
    fn filters_macos_provenance_xattr_from_reported_count() {
        assert!(!reportable_xattr_name(OsStr::new("com.apple.provenance")));
        assert!(reportable_xattr_name(OsStr::new(test_xattr_name())));
    }

    #[test]
    fn captures_file_flag_status_as_unsupported_until_platform_capture_exists() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.txt");
        fs::write(&path, b"hello").expect("write file");

        let metadata = capture_metadata(&path).expect("metadata");

        assert_eq!(metadata.extensions.file_flags, MetadataValue::Unsupported);
    }

    #[test]
    fn captures_windows_attribute_status_as_unsupported_until_platform_capture_exists() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("sample.txt");
        fs::write(&path, b"hello").expect("write file");

        let metadata = capture_metadata(&path).expect("metadata");

        assert_eq!(
            metadata.extensions.windows_attributes,
            MetadataValue::Unsupported
        );
    }

    #[cfg(unix)]
    #[test]
    fn captures_symlink_target_without_following_it() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        fs::write(&target, b"target").expect("write target");
        symlink("target.txt", &link).expect("symlink");

        let metadata = capture_metadata(&link).expect("metadata");

        assert_eq!(metadata.kind, EntryKind::Symlink);
        assert_eq!(metadata.size_bytes, None);
        assert_eq!(
            metadata.symlink_target,
            MetadataValue::Captured(PathBuf::from("target.txt"))
        );
    }

    #[test]
    fn reports_missing_path_with_path_context() {
        let temp = tempfile::tempdir().expect("tempdir");
        let missing = temp.path().join("missing");

        let error = capture_metadata(&missing).expect_err("missing path");

        assert!(error.to_string().contains("missing"));
    }

    #[test]
    fn classifies_normalized_relative_paths_and_parent_components() {
        let relative = Path::new("dir").join("file.txt");
        let facts = path_facts(&relative);

        assert!(facts.normalized_relative);
        assert!(!facts.has_parent_component);
        assert!(!facts.has_root_or_prefix);
        assert_eq!(facts.segment_count, 2);

        let escaping = Path::new("dir").join("..").join("file.txt");
        let facts = path_facts(&escaping);

        assert!(!facts.normalized_relative);
        assert!(facts.has_parent_component);
    }

    #[test]
    fn detects_windows_reserved_names_without_host_claims() {
        assert!(is_windows_reserved_name(OsStr::new("CON")));
        assert!(is_windows_reserved_name(OsStr::new("nul.txt")));
        assert!(is_windows_reserved_name(OsStr::new("LPT9")));
        assert!(!is_windows_reserved_name(OsStr::new("COM10")));
        assert!(!is_windows_reserved_name(OsStr::new("regular.txt")));

        let facts = path_facts(Path::new("backup").join("aux.log"));
        assert!(facts.has_windows_reserved_name);
    }

    #[test]
    fn probes_observed_case_behavior_for_temp_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let behavior = probe_case_behavior(temp.path()).expect("probe case behavior");

        assert!(matches!(
            behavior,
            CaseBehavior::CaseSensitive | CaseBehavior::CaseInsensitive
        ));
    }

    #[test]
    fn captures_metadata_for_long_relative_tree_where_filesystem_allows_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut directory = temp.path().to_path_buf();
        for index in 0..8 {
            directory.push(format!("segment-{index:02}-fileferry"));
        }
        fs::create_dir_all(&directory).expect("create long directory tree");
        let file = directory.join("sample.txt");
        fs::write(&file, b"long path").expect("write long path file");

        let metadata = capture_metadata(&file).expect("long path metadata");

        assert_eq!(metadata.kind, EntryKind::RegularFile);
        assert_eq!(metadata.size_bytes, Some(9));
    }

    #[cfg(unix)]
    #[test]
    fn reports_permission_denied_when_parent_search_is_denied() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().expect("tempdir");
        let locked_dir = temp.path().join("locked");
        fs::create_dir(&locked_dir).expect("create locked dir");
        let child = locked_dir.join("child.txt");
        fs::write(&child, b"child").expect("write child");
        let original = fs::metadata(&locked_dir)
            .expect("locked metadata")
            .permissions();

        fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o000))
            .expect("deny parent search");
        let result = capture_metadata(&child);
        fs::set_permissions(&locked_dir, original).expect("restore parent permissions");

        if let Err(PlatformError::MetadataRead { source, .. }) = result {
            assert_eq!(source.kind(), io::ErrorKind::PermissionDenied);
        }
    }

    fn test_xattr_name() -> &'static str {
        if cfg!(target_os = "macos") {
            "com.fileferry.test"
        } else {
            "user.fileferry_test"
        }
    }
}
