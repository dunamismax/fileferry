//! Core repository, snapshot, backup, restore, and check orchestration.

use fastcdc::v2020::{
    AVERAGE_MAX, AVERAGE_MIN, FastCDC, MAXIMUM_MAX, MAXIMUM_MIN, MINIMUM_MAX, MINIMUM_MIN,
};
use fileferry_crypto::{
    AeadAlgorithm, CryptoError, EncryptedObject, KeyPurpose, MasterKey, ObjectContext, ObjectKind,
    encrypt_object, keyed_content_id,
};
use fileferry_platform::{EntryKind, EntryMetadata, PlatformError, capture_metadata};
use fileferry_storage::{ObjectKey, ObjectStore, PutStatus, StorageError};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("source root {path} is not absolute")]
    SourceRootNotAbsolute { path: PathBuf },

    #[error("source root {path} could not be read")]
    SourceRootRead {
        path: PathBuf,
        #[source]
        source: PlatformError,
    },

    #[error("directory {path} could not be read")]
    DirectoryRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("directory entry in {path} could not be read")]
    DirectoryEntryRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("metadata for {path} could not be captured")]
    MetadataCapture {
        path: PathBuf,
        #[source]
        source: PlatformError,
    },

    #[error("chunking configuration is invalid: {reason}")]
    InvalidChunkingConfig { reason: &'static str },

    #[error("backup pipeline configuration is invalid: {reason}")]
    InvalidBackupPipelineConfig { reason: &'static str },

    #[error("file {path} could not be read")]
    FileRead {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("chunk range for {path} is invalid")]
    InvalidChunkRange { path: PathBuf },

    #[error("chunk for {path} could not be compressed")]
    Compression {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("repository object could not be encrypted")]
    Encryption {
        #[source]
        source: CryptoError,
    },

    #[error("repository metadata could not be serialized")]
    Serialization {
        #[source]
        source: serde_json::Error,
    },

    #[error("repository object key could not be created")]
    ObjectKey {
        #[source]
        source: StorageError,
    },

    #[error("repository object write failed")]
    Storage {
        #[source]
        source: StorageError,
    },
}

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct SourceEntry {
    pub root: PathBuf,
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub metadata: EntryMetadata,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceWalker {
    exclusion_rules: Vec<ExclusionRule>,
}

impl SourceWalker {
    pub fn new(exclusion_rules: Vec<ExclusionRule>) -> Self {
        Self { exclusion_rules }
    }

    pub fn walk(&self, roots: &[PathBuf]) -> CoreResult<Vec<SourceEntry>> {
        let mut entries = Vec::new();

        for root in roots {
            self.walk_root(root, &mut entries)?;
        }

        Ok(entries)
    }

    fn walk_root(&self, root: &Path, entries: &mut Vec<SourceEntry>) -> CoreResult<()> {
        if !root.is_absolute() {
            return Err(CoreError::SourceRootNotAbsolute {
                path: root.to_path_buf(),
            });
        }

        let root_metadata = capture_metadata(root).map_err(|source| CoreError::SourceRootRead {
            path: root.to_path_buf(),
            source,
        })?;
        let root = root.to_path_buf();
        entries.push(SourceEntry {
            root: root.clone(),
            path: root.clone(),
            relative_path: PathBuf::new(),
            metadata: root_metadata.clone(),
        });

        if root_metadata.kind != EntryKind::Directory {
            return Ok(());
        }

        let mut pending = VecDeque::from([root.clone()]);
        while let Some(directory) = pending.pop_front() {
            let mut children = read_sorted_children(&directory)?;

            for child in children.drain(..) {
                let relative_path = child
                    .strip_prefix(&root)
                    .expect("walked children must stay under root")
                    .to_path_buf();
                if self.is_excluded(&relative_path) {
                    continue;
                }

                let metadata =
                    capture_metadata(&child).map_err(|source| CoreError::MetadataCapture {
                        path: child.clone(),
                        source,
                    })?;
                if metadata.kind == EntryKind::Directory {
                    pending.push_back(child.clone());
                }

                entries.push(SourceEntry {
                    root: root.clone(),
                    path: child,
                    relative_path,
                    metadata,
                });
            }
        }

        Ok(())
    }

    fn is_excluded(&self, relative_path: &Path) -> bool {
        self.exclusion_rules
            .iter()
            .any(|rule| rule.matches(relative_path))
    }
}

pub const DEFAULT_MIN_CHUNK_SIZE: usize = 512 * 1024;
pub const DEFAULT_AVG_CHUNK_SIZE: usize = 1024 * 1024;
pub const DEFAULT_MAX_CHUNK_SIZE: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ChunkingConfig {
    pub min_size: usize,
    pub avg_size: usize,
    pub max_size: usize,
}

impl ChunkingConfig {
    pub const fn new(min_size: usize, avg_size: usize, max_size: usize) -> Self {
        Self {
            min_size,
            avg_size,
            max_size,
        }
    }

    pub fn validate(self) -> CoreResult<()> {
        if self.min_size < MINIMUM_MIN {
            return Err(CoreError::InvalidChunkingConfig {
                reason: "minimum chunk size is below the FastCDC lower bound",
            });
        }
        if self.min_size > MINIMUM_MAX {
            return Err(CoreError::InvalidChunkingConfig {
                reason: "minimum chunk size is above the FastCDC upper bound",
            });
        }
        if self.avg_size < AVERAGE_MIN {
            return Err(CoreError::InvalidChunkingConfig {
                reason: "average chunk size is below the FastCDC lower bound",
            });
        }
        if self.avg_size > AVERAGE_MAX {
            return Err(CoreError::InvalidChunkingConfig {
                reason: "average chunk size is above the FastCDC upper bound",
            });
        }
        if self.max_size < MAXIMUM_MIN {
            return Err(CoreError::InvalidChunkingConfig {
                reason: "maximum chunk size is below the FastCDC lower bound",
            });
        }
        if self.max_size > MAXIMUM_MAX {
            return Err(CoreError::InvalidChunkingConfig {
                reason: "maximum chunk size is above the FastCDC upper bound",
            });
        }
        if self.min_size > self.avg_size {
            return Err(CoreError::InvalidChunkingConfig {
                reason: "minimum chunk size must be less than or equal to average chunk size",
            });
        }
        if self.avg_size > self.max_size {
            return Err(CoreError::InvalidChunkingConfig {
                reason: "average chunk size must be less than or equal to maximum chunk size",
            });
        }

        Ok(())
    }
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self::new(
            DEFAULT_MIN_CHUNK_SIZE,
            DEFAULT_AVG_CHUNK_SIZE,
            DEFAULT_MAX_CHUNK_SIZE,
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ContentChunk {
    pub offset: u64,
    pub length: u64,
    pub gear_hash: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContentChunker {
    config: ChunkingConfig,
}

impl ContentChunker {
    pub fn new(config: ChunkingConfig) -> CoreResult<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub fn config(&self) -> ChunkingConfig {
        self.config
    }

    pub fn chunk_bytes(&self, bytes: &[u8]) -> Vec<ContentChunk> {
        FastCDC::new(
            bytes,
            self.config.min_size,
            self.config.avg_size,
            self.config.max_size,
        )
        .map(|chunk| ContentChunk {
            offset: chunk.offset as u64,
            length: chunk.length as u64,
            gear_hash: chunk.hash,
        })
        .collect()
    }
}

pub const DEFAULT_ZSTD_COMPRESSION_LEVEL: i32 = 3;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct BackupPipelineConfig {
    pub chunking: ChunkingConfig,
    pub compression_level: i32,
    pub repository_id: String,
}

impl BackupPipelineConfig {
    pub fn new(repository_id: impl Into<String>) -> Self {
        Self {
            chunking: ChunkingConfig::default(),
            compression_level: DEFAULT_ZSTD_COMPRESSION_LEVEL,
            repository_id: repository_id.into(),
        }
    }

    pub fn validate(&self) -> CoreResult<()> {
        self.chunking.validate()?;
        if self.repository_id.is_empty() {
            return Err(CoreError::InvalidBackupPipelineConfig {
                reason: "repository id must not be empty",
            });
        }
        if self.repository_id.as_bytes().contains(&0) {
            return Err(CoreError::InvalidBackupPipelineConfig {
                reason: "repository id must not contain NUL",
            });
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BackupRequest {
    pub roots: Vec<PathBuf>,
    pub exclusion_rules: Vec<ExclusionRule>,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct BackupPipeline {
    config: BackupPipelineConfig,
    chunker: ContentChunker,
}

impl BackupPipeline {
    pub fn new(config: BackupPipelineConfig) -> CoreResult<Self> {
        config.validate()?;
        let chunker = ContentChunker::new(config.chunking)?;
        Ok(Self { config, chunker })
    }

    pub fn config(&self) -> &BackupPipelineConfig {
        &self.config
    }

    pub async fn write_snapshot(
        &self,
        store: &dyn ObjectStore,
        master_key: &MasterKey,
        request: BackupRequest,
    ) -> CoreResult<SnapshotWriteResult> {
        let entries = SourceWalker::new(request.exclusion_rules)
            .walk(&request.roots)?
            .into_iter()
            .map(ManifestEntry::from_source_entry)
            .collect::<Vec<_>>();

        let repository_context = self.config.repository_id.as_bytes();
        let chunk_key = master_key
            .derive_subkey(KeyPurpose::ChunkData, repository_context)
            .map_err(|source| CoreError::Encryption { source })?;
        let index_key = master_key
            .derive_subkey(KeyPurpose::Index, repository_context)
            .map_err(|source| CoreError::Encryption { source })?;
        let manifest_key = master_key
            .derive_subkey(KeyPurpose::SnapshotMetadata, repository_context)
            .map_err(|source| CoreError::Encryption { source })?;

        let mut manifest_entries = Vec::with_capacity(entries.len());
        let mut index_entries = Vec::new();
        let mut chunk_objects_written = 0_usize;

        for mut entry in entries {
            if entry.metadata.kind == EntryKind::RegularFile {
                let file_bytes = fs::read(&entry.path).map_err(|source| CoreError::FileRead {
                    path: entry.path.clone(),
                    source,
                })?;
                for chunk in self.chunker.chunk_bytes(&file_bytes) {
                    let start = usize::try_from(chunk.offset).map_err(|_| {
                        CoreError::InvalidChunkRange {
                            path: entry.path.clone(),
                        }
                    })?;
                    let length = usize::try_from(chunk.length).map_err(|_| {
                        CoreError::InvalidChunkRange {
                            path: entry.path.clone(),
                        }
                    })?;
                    let end = start
                        .checked_add(length)
                        .filter(|end| *end <= file_bytes.len())
                        .ok_or_else(|| CoreError::InvalidChunkRange {
                            path: entry.path.clone(),
                        })?;
                    let plaintext = &file_bytes[start..end];
                    let chunk_id = hex_bytes(
                        &keyed_content_id(
                            master_key,
                            KeyPurpose::ChunkIdentity,
                            repository_context,
                            plaintext,
                        )
                        .map_err(|source| CoreError::Encryption { source })?,
                    );
                    let object_key = object_key_for_id("objects/chunk", &chunk_id)?;
                    let compressed = zstd::bulk::compress(plaintext, self.config.compression_level)
                        .map_err(|source| CoreError::Compression {
                            path: entry.path.clone(),
                            source,
                        })?;
                    let encrypted = encrypt_repository_object(
                        &chunk_key,
                        ObjectKind::Chunk,
                        &object_key,
                        &compressed,
                    )?;

                    if !store
                        .exists(&object_key)
                        .await
                        .map_err(|source| CoreError::Storage { source })?
                    {
                        match store
                            .put_if_absent(&object_key, &encrypted)
                            .await
                            .map_err(|source| CoreError::Storage { source })?
                        {
                            PutStatus::Created => chunk_objects_written += 1,
                            PutStatus::AlreadyPresent => {}
                        }
                    }

                    let chunk_ref = ManifestChunkRef {
                        chunk_id: chunk_id.clone(),
                        object_key: object_key.as_str().to_owned(),
                        offset: chunk.offset,
                        length: chunk.length,
                    };
                    entry.chunks.push(chunk_ref.clone());
                    index_entries.push(ChunkIndexEntry {
                        chunk_id,
                        object_key: object_key.as_str().to_owned(),
                        plaintext_length: chunk.length,
                        compressed_length: compressed.len() as u64,
                        stored_length: encrypted.len() as u64,
                        compression: CompressionAlgorithm::Zstd,
                        aead: RepositoryAeadAlgorithm::XChaCha20Poly1305,
                    });
                }
            }
            manifest_entries.push(entry);
        }

        index_entries.sort_by(|left, right| left.chunk_id.cmp(&right.chunk_id));
        index_entries.dedup_by(|left, right| left.chunk_id == right.chunk_id);

        let index_id = content_id_for_metadata(
            master_key,
            KeyPurpose::Index,
            repository_context,
            &index_entries,
        )?;
        let index = ChunkIndex {
            schema_version: 0,
            index_id: index_id.clone(),
            chunks: index_entries,
        };
        let index_object = object_key_for_id("objects/index", &index_id)?;
        write_encrypted_json_object(store, &index_key, ObjectKind::Index, &index_object, &index)
            .await?;

        let manifest_body = SnapshotManifestBody {
            tags: request.tags,
            entries: manifest_entries,
            index_ids: vec![index_id.clone()],
        };
        let snapshot_id = content_id_for_metadata(
            master_key,
            KeyPurpose::SnapshotMetadata,
            repository_context,
            &manifest_body,
        )?;
        let manifest = SnapshotManifest {
            schema_version: 0,
            snapshot_id: snapshot_id.clone(),
            body: manifest_body,
        };
        let manifest_object = object_key_for_id("objects/manifest", &snapshot_id)?;
        write_encrypted_json_object(
            store,
            &manifest_key,
            ObjectKind::SnapshotManifest,
            &manifest_object,
            &manifest,
        )
        .await?;

        Ok(SnapshotWriteResult {
            snapshot_id,
            manifest_object,
            index_object,
            chunk_objects_written,
            entries: manifest.body.entries.len(),
            chunks: index.chunks.len(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotWriteResult {
    pub snapshot_id: String,
    pub manifest_object: ObjectKey,
    pub index_object: ObjectKey,
    pub chunk_objects_written: usize,
    pub entries: usize,
    pub chunks: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct SnapshotManifest {
    pub schema_version: u16,
    pub snapshot_id: String,
    pub body: SnapshotManifestBody,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct SnapshotManifestBody {
    pub tags: Vec<String>,
    pub entries: Vec<ManifestEntry>,
    pub index_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ManifestEntry {
    pub root: PathBuf,
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub metadata: EntryMetadata,
    pub chunks: Vec<ManifestChunkRef>,
}

impl ManifestEntry {
    fn from_source_entry(entry: SourceEntry) -> Self {
        Self {
            root: entry.root,
            path: entry.path,
            relative_path: entry.relative_path,
            metadata: entry.metadata,
            chunks: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ManifestChunkRef {
    pub chunk_id: String,
    pub object_key: String,
    pub offset: u64,
    pub length: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ChunkIndex {
    pub schema_version: u16,
    pub index_id: String,
    pub chunks: Vec<ChunkIndexEntry>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ChunkIndexEntry {
    pub chunk_id: String,
    pub object_key: String,
    pub plaintext_length: u64,
    pub compressed_length: u64,
    pub stored_length: u64,
    pub compression: CompressionAlgorithm,
    pub aead: RepositoryAeadAlgorithm,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionAlgorithm {
    Zstd,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryAeadAlgorithm {
    XChaCha20Poly1305,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct StoredEncryptedObject {
    algorithm: RepositoryAeadAlgorithm,
    nonce: [u8; fileferry_crypto::XCHACHA20_POLY1305_NONCE_LEN],
    ciphertext: Vec<u8>,
}

fn encrypt_repository_object(
    key: &fileferry_crypto::Subkey,
    kind: ObjectKind,
    object_key: &ObjectKey,
    plaintext: &[u8],
) -> CoreResult<Vec<u8>> {
    let context = ObjectContext::new(kind, object_key.as_str())
        .map_err(|source| CoreError::Encryption { source })?;
    let encrypted = encrypt_object(key, &context, plaintext)
        .map_err(|source| CoreError::Encryption { source })?;
    encode_encrypted_object(encrypted)
}

async fn write_encrypted_json_object<T: Serialize>(
    store: &dyn ObjectStore,
    key: &fileferry_crypto::Subkey,
    kind: ObjectKind,
    object_key: &ObjectKey,
    value: &T,
) -> CoreResult<()> {
    let plaintext =
        serde_json::to_vec(value).map_err(|source| CoreError::Serialization { source })?;
    let encrypted = encrypt_repository_object(key, kind, object_key, &plaintext)?;
    store
        .put_if_absent(object_key, &encrypted)
        .await
        .map_err(|source| CoreError::Storage { source })?;
    Ok(())
}

fn encode_encrypted_object(encrypted: EncryptedObject) -> CoreResult<Vec<u8>> {
    let algorithm = match encrypted.algorithm {
        AeadAlgorithm::XChaCha20Poly1305 => RepositoryAeadAlgorithm::XChaCha20Poly1305,
    };
    serde_json::to_vec(&StoredEncryptedObject {
        algorithm,
        nonce: encrypted.nonce,
        ciphertext: encrypted.ciphertext,
    })
    .map_err(|source| CoreError::Serialization { source })
}

fn content_id_for_metadata<T: Serialize>(
    master_key: &MasterKey,
    purpose: KeyPurpose,
    context: &[u8],
    value: &T,
) -> CoreResult<String> {
    let bytes = serde_json::to_vec(value).map_err(|source| CoreError::Serialization { source })?;
    keyed_content_id(master_key, purpose, context, &bytes)
        .map(|id| hex_bytes(&id))
        .map_err(|source| CoreError::Encryption { source })
}

fn object_key_for_id(group: &str, id: &str) -> CoreResult<ObjectKey> {
    let prefix = id.get(..2).ok_or(CoreError::InvalidBackupPipelineConfig {
        reason: "object id must be at least two characters",
    })?;
    ObjectKey::new(format!("{group}/{prefix}/{id}"))
        .map_err(|source| CoreError::ObjectKey { source })
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExclusionRule {
    pattern: String,
    segments: Vec<String>,
    directory_prefix: bool,
}

impl ExclusionRule {
    pub fn new(pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        let directory_prefix = pattern.ends_with('/');
        let normalized = pattern.trim_matches('/').replace('\\', "/");
        let segments = normalized
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect();

        Self {
            pattern,
            segments,
            directory_prefix,
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn matches(&self, relative_path: &Path) -> bool {
        let path_segments = path_segments(relative_path);
        if path_segments.is_empty() || self.segments.is_empty() {
            return false;
        }

        if self.directory_prefix && path_segments.len() < self.segments.len() {
            return false;
        }

        if !self.pattern.contains('/') && !self.pattern.contains('\\') {
            return path_segments
                .iter()
                .any(|segment| wildcard_match(&self.segments[0], segment));
        }

        if self.directory_prefix {
            return path_segments_match(&self.segments, &path_segments[..self.segments.len()]);
        }

        path_segments_match(&self.segments, &path_segments)
    }
}

fn read_sorted_children(directory: &Path) -> CoreResult<Vec<PathBuf>> {
    let read_dir = fs::read_dir(directory).map_err(|source| CoreError::DirectoryRead {
        path: directory.to_path_buf(),
        source,
    })?;
    let mut children = Vec::new();

    for entry in read_dir {
        let entry = entry.map_err(|source| CoreError::DirectoryEntryRead {
            path: directory.to_path_buf(),
            source,
        })?;
        children.push(entry.path());
    }

    children.sort();
    Ok(children)
}

fn path_segments(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect()
}

fn path_segments_match(pattern: &[String], path: &[String]) -> bool {
    match (pattern.split_first(), path.split_first()) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some((pattern_head, pattern_tail)), _) if pattern_head == "**" => {
            path_segments_match(pattern_tail, path)
                || path
                    .split_first()
                    .is_some_and(|(_, path_tail)| path_segments_match(pattern, path_tail))
        }
        (Some(_), None) => false,
        (Some((pattern_head, pattern_tail)), Some((path_head, path_tail))) => {
            wildcard_match(pattern_head, path_head) && path_segments_match(pattern_tail, path_tail)
        }
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    let mut remaining = value;
    let mut parts = pattern.split('*').peekable();
    let starts_with_wildcard = pattern.starts_with('*');
    let ends_with_wildcard = pattern.ends_with('*');

    if let Some(first) = parts.next() {
        if !first.is_empty() {
            if !remaining.starts_with(first) {
                return false;
            }
            remaining = &remaining[first.len()..];
        } else if !starts_with_wildcard {
            return false;
        }
    }

    while let Some(part) = parts.next() {
        if part.is_empty() {
            continue;
        }

        match remaining.find(part) {
            Some(index) => {
                remaining = &remaining[index + part.len()..];
                if parts.peek().is_none() && !ends_with_wildcard {
                    return remaining.is_empty();
                }
            }
            None => return false,
        }
    }

    ends_with_wildcard || remaining.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relative_entries(entries: &[SourceEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| {
                if entry.relative_path.as_os_str().is_empty() {
                    ".".to_owned()
                } else {
                    entry.relative_path.display().to_string()
                }
            })
            .collect()
    }

    #[test]
    fn walks_sources_in_deterministic_relative_order() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("b")).expect("create b");
        fs::create_dir(temp.path().join("a")).expect("create a");
        fs::write(temp.path().join("b/file.txt"), b"b").expect("write b");
        fs::write(temp.path().join("a/file.txt"), b"a").expect("write a");

        let entries = SourceWalker::default()
            .walk(&[temp.path().to_path_buf()])
            .expect("walk");

        assert_eq!(
            relative_entries(&entries),
            vec![".", "a", "b", "a/file.txt", "b/file.txt"]
        );
    }

    #[test]
    fn excludes_matching_files_and_prunes_matching_directories() {
        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(temp.path().join("project/target")).expect("create target");
        fs::create_dir_all(temp.path().join("project/src")).expect("create src");
        fs::write(temp.path().join("project/target/build.log"), b"log").expect("write log");
        fs::write(temp.path().join("project/src/main.rs"), b"fn main() {}").expect("write main");
        fs::write(temp.path().join("project/src/main.tmp"), b"tmp").expect("write tmp");

        let walker = SourceWalker::new(vec![
            ExclusionRule::new("**/target"),
            ExclusionRule::new("*.tmp"),
        ]);
        let entries = walker.walk(&[temp.path().to_path_buf()]).expect("walk");

        assert_eq!(
            relative_entries(&entries),
            vec![".", "project", "project/src", "project/src/main.rs"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn records_symlinks_without_following_directory_targets() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().expect("tempdir");
        fs::create_dir(temp.path().join("real")).expect("create real");
        fs::write(temp.path().join("real/nested.txt"), b"nested").expect("write nested");
        symlink("real", temp.path().join("link")).expect("symlink");

        let entries = SourceWalker::default()
            .walk(&[temp.path().to_path_buf()])
            .expect("walk");

        assert_eq!(
            relative_entries(&entries),
            vec![".", "link", "real", "real/nested.txt"]
        );
        let link = entries
            .iter()
            .find(|entry| entry.relative_path == Path::new("link"))
            .expect("link entry");
        assert_eq!(link.metadata.kind, EntryKind::Symlink);
    }

    #[test]
    fn rejects_relative_roots() {
        let error = SourceWalker::default()
            .walk(&[PathBuf::from("relative")])
            .expect_err("relative root");

        assert!(matches!(error, CoreError::SourceRootNotAbsolute { .. }));
    }

    #[test]
    fn wildcard_patterns_match_expected_paths() {
        assert!(ExclusionRule::new("**/.git").matches(Path::new("src/.git")));
        assert!(ExclusionRule::new("*.tmp").matches(Path::new("src/cache.tmp")));
        assert!(ExclusionRule::new("node_modules").matches(Path::new("app/node_modules")));
        assert!(!ExclusionRule::new("*.tmp").matches(Path::new("src/cache.txt")));
    }

    #[test]
    fn chunking_config_validates_fastcdc_bounds_and_order() {
        assert!(ChunkingConfig::new(64, 256, 1024).validate().is_ok());
        assert!(matches!(
            ChunkingConfig::new(63, 256, 1024).validate(),
            Err(CoreError::InvalidChunkingConfig { .. })
        ));
        assert!(matches!(
            ChunkingConfig::new(512, 256, 1024).validate(),
            Err(CoreError::InvalidChunkingConfig { .. })
        ));
        assert!(matches!(
            ChunkingConfig::new(64, 2048, 1024).validate(),
            Err(CoreError::InvalidChunkingConfig { .. })
        ));
    }

    #[test]
    fn content_chunker_returns_deterministic_ranges_covering_input() {
        let config = ChunkingConfig::new(64, 256, 1024);
        let chunker = ContentChunker::new(config).expect("valid chunker");
        let bytes = (0..16_384)
            .map(|index| ((index * 31 + index / 7) % 251) as u8)
            .collect::<Vec<_>>();

        let first = chunker.chunk_bytes(&bytes);
        let second = chunker.chunk_bytes(&bytes);

        assert_eq!(first, second);
        assert!(!first.is_empty());
        assert_eq!(first.first().expect("first chunk").offset, 0);

        let mut cursor = 0_u64;
        for chunk in &first {
            assert_eq!(chunk.offset, cursor);
            assert!(chunk.length > 0);
            assert!(chunk.length <= config.max_size as u64);
            cursor += chunk.length;
        }
        assert_eq!(cursor, bytes.len() as u64);
    }

    #[test]
    fn content_chunker_keeps_small_inputs_as_one_chunk() {
        let chunker =
            ContentChunker::new(ChunkingConfig::new(64, 256, 1024)).expect("valid chunker");

        assert_eq!(chunker.chunk_bytes(&[]), Vec::new());
        assert_eq!(
            chunker.chunk_bytes(b"short"),
            vec![ContentChunk {
                offset: 0,
                length: 5,
                gear_hash: 0,
            }]
        );
    }

    #[test]
    fn backup_pipeline_rejects_empty_repository_context() {
        let error = BackupPipeline::new(BackupPipelineConfig {
            chunking: ChunkingConfig::new(64, 256, 1024),
            compression_level: DEFAULT_ZSTD_COMPRESSION_LEVEL,
            repository_id: String::new(),
        })
        .expect_err("empty repository id");

        assert!(matches!(
            error,
            CoreError::InvalidBackupPipelineConfig { .. }
        ));
    }

    #[tokio::test]
    async fn backup_pipeline_writes_encrypted_chunks_index_and_manifest() {
        use fileferry_storage::ObjectKeyPrefix;
        use fileferry_testkit::FakeObjectStore;

        let temp = tempfile::tempdir().expect("tempdir");
        fs::write(temp.path().join("one.txt"), b"same content").expect("write one");
        fs::write(temp.path().join("two.txt"), b"same content").expect("write two");

        let config = BackupPipelineConfig {
            chunking: ChunkingConfig::new(64, 256, 1024),
            compression_level: DEFAULT_ZSTD_COMPRESSION_LEVEL,
            repository_id: "repo-test-id".to_owned(),
        };
        let pipeline = BackupPipeline::new(config).expect("pipeline");
        let store = FakeObjectStore::new();
        let master_key = MasterKey::generate();

        let result = pipeline
            .write_snapshot(
                &store,
                &master_key,
                BackupRequest {
                    roots: vec![temp.path().to_path_buf()],
                    exclusion_rules: Vec::new(),
                    tags: vec!["laptop".to_owned()],
                },
            )
            .await
            .expect("snapshot write");

        assert_eq!(result.entries, 3);
        assert_eq!(result.chunks, 1);
        assert_eq!(result.chunk_objects_written, 1);
        assert_eq!(store.object_count().await, 3);

        let keys = store
            .list_prefix(&ObjectKeyPrefix::root())
            .await
            .expect("list objects");
        let rendered_keys = keys
            .iter()
            .map(|key| key.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered_keys.contains("objects/chunk/"));
        assert!(rendered_keys.contains("objects/index/"));
        assert!(rendered_keys.contains("objects/manifest/"));
        assert!(!rendered_keys.contains("one.txt"));
        assert!(!rendered_keys.contains("two.txt"));

        let manifest_bytes = store.get(&result.manifest_object).await.expect("manifest");
        let rendered_manifest = String::from_utf8_lossy(&manifest_bytes);
        assert!(!rendered_manifest.contains("one.txt"));
        assert!(!rendered_manifest.contains("two.txt"));
        assert!(!rendered_manifest.contains("laptop"));
    }
}
