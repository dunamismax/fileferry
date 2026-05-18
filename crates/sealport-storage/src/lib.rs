//! Local and object storage abstractions and backend implementations.

use std::{
    fmt,
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    sync::atomic::{AtomicU64, Ordering},
};

use futures_util::TryStreamExt;
use object_store::{
    Error as ObjectStoreError, ObjectStore as ObjectStoreBackend, ObjectStoreExt, PutMode,
    aws::{AmazonS3Builder, S3ConditionalPut},
    path::Path as ObjectStorePath,
};
use secrecy::{ExposeSecret, SecretString};
use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};

pub type StorageResult<T> = Result<T, StorageError>;
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = StorageResult<T>> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("object key {value:?} is invalid: {reason}")]
    InvalidObjectKey { value: String, reason: &'static str },

    #[error("object {key} already exists with different contents")]
    ObjectAlreadyExists { key: ObjectKey },

    #[error("object {key} was not found")]
    ObjectNotFound { key: ObjectKey },

    #[error("{operation} failed")]
    Io {
        operation: &'static str,
        #[source]
        source: io::Error,
    },

    #[error("{backend:?} backend configuration failed: {reason}")]
    BackendConfig {
        backend: BackendKind,
        reason: String,
    },

    #[error("{operation} failed for object {key}")]
    ObjectIo {
        operation: &'static str,
        key: ObjectKey,
        #[source]
        source: io::Error,
    },

    #[error("{operation} failed for object {key} on {backend:?}")]
    BackendObject {
        backend: BackendKind,
        operation: &'static str,
        key: ObjectKey,
        #[source]
        source: ObjectStoreError,
    },

    #[error("{operation} failed on {backend:?}")]
    Backend {
        backend: BackendKind,
        operation: &'static str,
        #[source]
        source: ObjectStoreError,
    },
}

impl StorageError {
    fn io(operation: &'static str, source: io::Error) -> Self {
        Self::Io { operation, source }
    }

    fn object_io(operation: &'static str, key: &ObjectKey, source: io::Error) -> Self {
        if source.kind() == io::ErrorKind::NotFound {
            Self::ObjectNotFound { key: key.clone() }
        } else {
            Self::ObjectIo {
                operation,
                key: key.clone(),
                source,
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageCapabilities {
    pub backend: BackendKind,
    pub conditional_create: bool,
    pub atomic_visibility: bool,
    pub strong_read_after_write: bool,
    pub delete: DeleteCapability,
    pub listing: ListingCapability,
}

impl StorageCapabilities {
    pub fn local_filesystem() -> Self {
        Self {
            backend: BackendKind::LocalFilesystem,
            conditional_create: true,
            atomic_visibility: true,
            strong_read_after_write: true,
            delete: DeleteCapability::Idempotent,
            listing: ListingCapability::Prefix,
        }
    }

    pub fn in_memory_fake() -> Self {
        Self {
            backend: BackendKind::InMemoryFake,
            conditional_create: true,
            atomic_visibility: true,
            strong_read_after_write: true,
            delete: DeleteCapability::Idempotent,
            listing: ListingCapability::Prefix,
        }
    }

    pub fn s3_compatible(conditional_create: bool) -> Self {
        Self {
            backend: BackendKind::S3Compatible,
            conditional_create,
            atomic_visibility: true,
            strong_read_after_write: false,
            delete: DeleteCapability::Idempotent,
            listing: ListingCapability::Prefix,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    LocalFilesystem,
    S3Compatible,
    InMemoryFake,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeleteCapability {
    Unsupported,
    BestEffort,
    Idempotent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListingCapability {
    Unsupported,
    Prefix,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PutStatus {
    Created,
    AlreadyPresent,
}

pub trait ObjectStore: Send + Sync {
    fn capabilities(&self) -> StorageCapabilities;

    fn put_if_absent<'a>(
        &'a self,
        key: &'a ObjectKey,
        bytes: &'a [u8],
    ) -> StorageFuture<'a, PutStatus>;

    fn get<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, Vec<u8>>;

    fn exists<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, bool>;

    fn delete<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, ()>;

    fn list_prefix<'a>(&'a self, prefix: &'a ObjectKeyPrefix) -> StorageFuture<'a, Vec<ObjectKey>>;
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectKey(String);

impl ObjectKey {
    pub fn new(value: impl Into<String>) -> StorageResult<Self> {
        let value = value.into();
        validate_key(&value, false)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn relative_path(&self) -> PathBuf {
        self.0.split('/').collect()
    }
}

impl fmt::Display for ObjectKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl TryFrom<&str> for ObjectKey {
    type Error = StorageError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectKeyPrefix(String);

impl ObjectKeyPrefix {
    pub fn root() -> Self {
        Self(String::new())
    }

    pub fn new(value: impl Into<String>) -> StorageResult<Self> {
        let value = value.into();
        validate_key(&value, true)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn relative_path(&self) -> PathBuf {
        self.0.split('/').filter(|part| !part.is_empty()).collect()
    }

    fn contains(&self, key: &ObjectKey) -> bool {
        if self.0.is_empty() {
            return true;
        }

        key.0 == self.0
            || key
                .0
                .strip_prefix(&self.0)
                .is_some_and(|remainder| remainder.starts_with('/'))
    }
}

impl TryFrom<&str> for ObjectKeyPrefix {
    type Error = StorageError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

fn validate_key(value: &str, allow_empty: bool) -> StorageResult<()> {
    if value.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(StorageError::InvalidObjectKey {
                value: value.to_owned(),
                reason: "key must not be empty",
            })
        };
    }

    if value.starts_with('/') || value.ends_with('/') {
        return Err(StorageError::InvalidObjectKey {
            value: value.to_owned(),
            reason: "key must be relative and must not end with a separator",
        });
    }

    if value.contains('\\') {
        return Err(StorageError::InvalidObjectKey {
            value: value.to_owned(),
            reason: "key must use forward slashes",
        });
    }

    for segment in value.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(StorageError::InvalidObjectKey {
                value: value.to_owned(),
                reason: "key segments must not be empty, '.', or '..'",
            });
        }

        if !segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'='))
        {
            return Err(StorageError::InvalidObjectKey {
                value: value.to_owned(),
                reason: "key segments may contain only ASCII letters, digits, '.', '_', '-', or '='",
            });
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
pub struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn object_path(&self, key: &ObjectKey) -> PathBuf {
        self.root.join(key.relative_path())
    }

    fn prefix_path(&self, prefix: &ObjectKeyPrefix) -> PathBuf {
        self.root.join(prefix.relative_path())
    }

    fn temp_dir(&self) -> PathBuf {
        self.root.join(".sealport-tmp")
    }

    fn temp_path(&self, key: &ObjectKey) -> PathBuf {
        static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);

        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        self.temp_dir().join(format!(
            "{}-{}-{id}.part",
            std::process::id(),
            key.as_str().replace('/', "_")
        ))
    }
}

#[derive(Clone)]
pub struct S3StoreConfig {
    bucket: String,
    region: String,
    endpoint: String,
    access_key_id: SecretString,
    secret_access_key: SecretString,
    root_prefix: ObjectKeyPrefix,
    conditional_create: bool,
}

impl fmt::Debug for S3StoreConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3StoreConfig")
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("endpoint", &self.endpoint)
            .field("access_key_id", &"[redacted]")
            .field("secret_access_key", &"[redacted]")
            .field("root_prefix", &self.root_prefix)
            .field("conditional_create", &self.conditional_create)
            .finish()
    }
}

impl S3StoreConfig {
    pub fn new(
        bucket: impl Into<String>,
        region: impl Into<String>,
        endpoint: impl Into<String>,
        access_key_id: impl Into<SecretString>,
        secret_access_key: impl Into<SecretString>,
        root_prefix: ObjectKeyPrefix,
    ) -> StorageResult<Self> {
        let bucket = bucket.into();
        let region = region.into();
        let endpoint = endpoint.into();

        validate_s3_config_value("bucket", &bucket)?;
        validate_s3_config_value("region", &region)?;
        validate_s3_endpoint(&endpoint)?;

        Ok(Self {
            bucket,
            region,
            endpoint,
            access_key_id: access_key_id.into(),
            secret_access_key: secret_access_key.into(),
            root_prefix,
            conditional_create: true,
        })
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }

    pub fn region(&self) -> &str {
        &self.region
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn root_prefix(&self) -> &ObjectKeyPrefix {
        &self.root_prefix
    }

    pub fn with_conditional_create(mut self, conditional_create: bool) -> Self {
        self.conditional_create = conditional_create;
        self
    }
}

#[derive(Clone)]
pub struct S3Store {
    inner: Arc<dyn ObjectStoreBackend>,
    root_prefix: ObjectKeyPrefix,
    conditional_create: bool,
}

impl fmt::Debug for S3Store {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("S3Store")
            .field("root_prefix", &self.root_prefix)
            .finish_non_exhaustive()
    }
}

impl S3Store {
    pub fn new(config: S3StoreConfig) -> StorageResult<Self> {
        let store = AmazonS3Builder::new()
            .with_bucket_name(config.bucket)
            .with_region(config.region)
            .with_endpoint(config.endpoint)
            .with_access_key_id(config.access_key_id.expose_secret())
            .with_secret_access_key(config.secret_access_key.expose_secret())
            .with_virtual_hosted_style_request(false)
            .with_conditional_put(S3ConditionalPut::ETagMatch)
            .with_disable_tagging(true)
            .build()
            .map_err(|source| StorageError::BackendConfig {
                backend: BackendKind::S3Compatible,
                reason: source.to_string(),
            })?;

        Ok(Self {
            inner: Arc::new(store),
            root_prefix: config.root_prefix,
            conditional_create: config.conditional_create,
        })
    }

    fn remote_path_for_key(&self, key: &ObjectKey) -> ObjectStorePath {
        if self.root_prefix.as_str().is_empty() {
            ObjectStorePath::from(key.as_str())
        } else {
            ObjectStorePath::from(format!("{}/{}", self.root_prefix.as_str(), key.as_str()))
        }
    }

    fn remote_path_for_prefix(&self, prefix: &ObjectKeyPrefix) -> Option<ObjectStorePath> {
        match (self.root_prefix.as_str(), prefix.as_str()) {
            ("", "") => None,
            ("", prefix) => Some(ObjectStorePath::from(prefix)),
            (root, "") => Some(ObjectStorePath::from(root)),
            (root, prefix) => Some(ObjectStorePath::from(format!("{root}/{prefix}"))),
        }
    }

    fn local_key_from_remote(&self, remote: &ObjectStorePath) -> Option<StorageResult<ObjectKey>> {
        let remote = remote.as_ref();
        let root = self.root_prefix.as_str();

        if root.is_empty() {
            return Some(ObjectKey::new(remote.to_owned()));
        }

        let remainder = remote.strip_prefix(root)?.strip_prefix('/')?;
        if remainder.is_empty() {
            None
        } else {
            Some(ObjectKey::new(remainder.to_owned()))
        }
    }
}

impl ObjectStore for S3Store {
    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::s3_compatible(self.conditional_create)
    }

    fn put_if_absent<'a>(
        &'a self,
        key: &'a ObjectKey,
        bytes: &'a [u8],
    ) -> StorageFuture<'a, PutStatus> {
        Box::pin(async move {
            let path = self.remote_path_for_key(key);
            if !self.conditional_create {
                match self.get(key).await {
                    Ok(existing) if existing == bytes => return Ok(PutStatus::AlreadyPresent),
                    Ok(_) => return Err(StorageError::ObjectAlreadyExists { key: key.clone() }),
                    Err(StorageError::ObjectNotFound { .. }) => {}
                    Err(error) => return Err(error),
                }

                return self
                    .inner
                    .put_opts(&path, bytes.to_vec().into(), PutMode::Overwrite.into())
                    .await
                    .map(|_| PutStatus::Created)
                    .map_err(|source| map_s3_object_error("put object", key, source));
            }

            match self
                .inner
                .put_opts(&path, bytes.to_vec().into(), PutMode::Create.into())
                .await
            {
                Ok(_) => Ok(PutStatus::Created),
                Err(ObjectStoreError::AlreadyExists { .. }) => {
                    let existing = self.get(key).await?;
                    if existing == bytes {
                        Ok(PutStatus::AlreadyPresent)
                    } else {
                        Err(StorageError::ObjectAlreadyExists { key: key.clone() })
                    }
                }
                Err(source) => Err(map_s3_object_error("put object", key, source)),
            }
        })
    }

    fn get<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, Vec<u8>> {
        Box::pin(async move {
            let path = self.remote_path_for_key(key);
            let object = self
                .inner
                .get(&path)
                .await
                .map_err(|source| map_s3_object_error("read object", key, source))?;
            let bytes = object
                .bytes()
                .await
                .map_err(|source| map_s3_object_error("read object bytes", key, source))?;
            Ok(bytes.to_vec())
        })
    }

    fn exists<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, bool> {
        Box::pin(async move {
            let path = self.remote_path_for_key(key);
            match self.inner.head(&path).await {
                Ok(_) => Ok(true),
                Err(ObjectStoreError::NotFound { .. }) => Ok(false),
                Err(source) => Err(map_s3_object_error("stat object", key, source)),
            }
        })
    }

    fn delete<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let path = self.remote_path_for_key(key);
            match self.inner.delete(&path).await {
                Ok(()) | Err(ObjectStoreError::NotFound { .. }) => Ok(()),
                Err(source) => Err(map_s3_object_error("delete object", key, source)),
            }
        })
    }

    fn list_prefix<'a>(&'a self, prefix: &'a ObjectKeyPrefix) -> StorageFuture<'a, Vec<ObjectKey>> {
        Box::pin(async move {
            let remote_prefix = self.remote_path_for_prefix(prefix);
            let mut stream = self.inner.list(remote_prefix.as_ref());
            let mut output = Vec::new();

            while let Some(meta) = stream
                .try_next()
                .await
                .map_err(|source| map_s3_backend_error("list objects", source))?
            {
                if let Some(key) = self.local_key_from_remote(&meta.location) {
                    output.push(key?);
                }
            }

            output.retain(|key| prefix.contains(key));
            output.sort();
            Ok(output)
        })
    }
}

fn validate_s3_config_value(name: &'static str, value: &str) -> StorageResult<()> {
    if value.trim().is_empty() {
        return Err(StorageError::BackendConfig {
            backend: BackendKind::S3Compatible,
            reason: format!("{name} must not be empty"),
        });
    }

    Ok(())
}

fn validate_s3_endpoint(endpoint: &str) -> StorageResult<()> {
    validate_s3_config_value("endpoint", endpoint)?;
    if !endpoint.starts_with("https://") {
        return Err(StorageError::BackendConfig {
            backend: BackendKind::S3Compatible,
            reason: "endpoint must be an https:// URL".to_owned(),
        });
    }

    Ok(())
}

fn map_s3_object_error(
    operation: &'static str,
    key: &ObjectKey,
    source: ObjectStoreError,
) -> StorageError {
    match source {
        ObjectStoreError::NotFound { .. } => StorageError::ObjectNotFound { key: key.clone() },
        source => StorageError::BackendObject {
            backend: BackendKind::S3Compatible,
            operation,
            key: key.clone(),
            source,
        },
    }
}

fn map_s3_backend_error(operation: &'static str, source: ObjectStoreError) -> StorageError {
    StorageError::Backend {
        backend: BackendKind::S3Compatible,
        operation,
        source,
    }
}

impl ObjectStore for LocalStore {
    fn capabilities(&self) -> StorageCapabilities {
        StorageCapabilities::local_filesystem()
    }

    fn put_if_absent<'a>(
        &'a self,
        key: &'a ObjectKey,
        bytes: &'a [u8],
    ) -> StorageFuture<'a, PutStatus> {
        Box::pin(async move {
            let path = self.object_path(key);
            let temp_path = self.temp_path(key);

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await.map_err(|source| {
                    StorageError::object_io("create object parent", key, source)
                })?;
            }

            fs::create_dir_all(self.temp_dir())
                .await
                .map_err(|source| StorageError::io("create temporary object directory", source))?;

            let mut temp_file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .await
                .map_err(|source| StorageError::io("create temporary object", source))?;
            temp_file
                .write_all(bytes)
                .await
                .map_err(|source| StorageError::io("write temporary object", source))?;
            temp_file
                .sync_all()
                .await
                .map_err(|source| StorageError::io("sync temporary object", source))?;
            drop(temp_file);

            match fs::hard_link(&temp_path, &path).await {
                Ok(()) => {
                    remove_temp(&temp_path).await?;
                    Ok(PutStatus::Created)
                }
                Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
                    remove_temp(&temp_path).await?;
                    let existing = read_existing_for_compare(key, &path).await?;
                    if existing == bytes {
                        Ok(PutStatus::AlreadyPresent)
                    } else {
                        Err(StorageError::ObjectAlreadyExists { key: key.clone() })
                    }
                }
                Err(source) => {
                    remove_temp(&temp_path).await?;
                    Err(StorageError::object_io("publish object", key, source))
                }
            }
        })
    }

    fn get<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, Vec<u8>> {
        Box::pin(async move {
            fs::read(self.object_path(key))
                .await
                .map_err(|source| StorageError::object_io("read object", key, source))
        })
    }

    fn exists<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, bool> {
        Box::pin(async move {
            match fs::metadata(self.object_path(key)).await {
                Ok(metadata) => Ok(metadata.is_file()),
                Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(false),
                Err(source) => Err(StorageError::object_io("stat object", key, source)),
            }
        })
    }

    fn delete<'a>(&'a self, key: &'a ObjectKey) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            match fs::remove_file(self.object_path(key)).await {
                Ok(()) => Ok(()),
                Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(source) => Err(StorageError::object_io("delete object", key, source)),
            }
        })
    }

    fn list_prefix<'a>(&'a self, prefix: &'a ObjectKeyPrefix) -> StorageFuture<'a, Vec<ObjectKey>> {
        Box::pin(async move {
            let root = self.root.clone();
            let start = self.prefix_path(prefix);
            let mut output = Vec::new();

            match fs::metadata(&start).await {
                Ok(metadata) if metadata.is_file() => {
                    push_object_path(&root, &start, &mut output)?;
                }
                Ok(metadata) if metadata.is_dir() => {
                    collect_files(&root, &start, &mut output).await?;
                }
                Ok(_) => {}
                Err(source) if source.kind() == io::ErrorKind::NotFound => {}
                Err(source) => return Err(StorageError::io("stat prefix", source)),
            }

            output
                .retain(|key| prefix.contains(key) && !key.as_str().starts_with(".sealport-tmp/"));
            output.sort();
            Ok(output)
        })
    }
}

async fn remove_temp(path: &Path) -> StorageResult<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StorageError::io("remove temporary object", source)),
    }
}

async fn read_existing_for_compare(key: &ObjectKey, path: &Path) -> StorageResult<Vec<u8>> {
    fs::read(path)
        .await
        .map_err(|source| StorageError::object_io("read existing object", key, source))
}

async fn collect_files(
    root: &Path,
    directory: &Path,
    output: &mut Vec<ObjectKey>,
) -> StorageResult<()> {
    let mut stack = vec![directory.to_path_buf()];

    while let Some(current) = stack.pop() {
        let mut entries = fs::read_dir(&current)
            .await
            .map_err(|source| StorageError::io("read object directory", source))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|source| StorageError::io("read object directory entry", source))?
        {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .await
                .map_err(|source| StorageError::io("read object file type", source))?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                push_object_path(root, &path, output)?;
            }
        }
    }

    Ok(())
}

fn push_object_path(root: &Path, path: &Path, output: &mut Vec<ObjectKey>) -> StorageResult<()> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| StorageError::InvalidObjectKey {
            value: path.display().to_string(),
            reason: "object path is outside the storage root",
        })?;
    let key = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");

    output.push(ObjectKey::new(key)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(value: &str) -> ObjectKey {
        ObjectKey::new(value).expect("valid object key")
    }

    #[test]
    fn object_keys_reject_path_escape_and_platform_separators() {
        for invalid in [
            "",
            "/chunks/a",
            "chunks/",
            "chunks//a",
            "chunks/../a",
            "chunks\\a",
        ] {
            assert!(ObjectKey::new(invalid).is_err(), "{invalid:?}");
        }

        assert_eq!(
            key("chunks/ab/cd.ef_01-02=03").as_str(),
            "chunks/ab/cd.ef_01-02=03"
        );
    }

    #[test]
    fn s3_config_debug_redacts_credentials() {
        let config = S3StoreConfig::new(
            "dev-bucket",
            "us-west-001",
            "https://s3.us-west-001.backblazeb2.com",
            "visible-key-id",
            "visible-secret-key",
            ObjectKeyPrefix::new("sealport/dev").expect("prefix"),
        )
        .expect("config");

        let debug = format!("{config:?}");
        assert!(debug.contains("dev-bucket"));
        assert!(debug.contains("us-west-001"));
        assert!(!debug.contains("visible-key-id"));
        assert!(!debug.contains("visible-secret-key"));
        assert_eq!(config.bucket(), "dev-bucket");
        assert_eq!(config.region(), "us-west-001");
        assert_eq!(config.endpoint(), "https://s3.us-west-001.backblazeb2.com");
    }

    #[test]
    fn s3_config_requires_https_endpoint() {
        let error = S3StoreConfig::new(
            "dev-bucket",
            "us-west-001",
            "http://s3.us-west-001.backblazeb2.com",
            "key-id",
            "secret",
            ObjectKeyPrefix::new("sealport/dev").expect("prefix"),
        )
        .expect_err("http endpoint");

        assert!(matches!(error, StorageError::BackendConfig { .. }));
    }

    #[tokio::test]
    async fn local_store_put_get_list_and_delete_round_trip() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = LocalStore::new(temp.path());
        let object = key("chunks/aa/blob");

        assert_eq!(
            store.capabilities(),
            StorageCapabilities::local_filesystem()
        );
        assert!(!store.exists(&object).await.expect("exists"));
        assert_eq!(
            store.put_if_absent(&object, b"sealed").await.expect("put"),
            PutStatus::Created
        );
        assert!(store.exists(&object).await.expect("exists"));
        assert_eq!(store.get(&object).await.expect("get"), b"sealed");
        assert_eq!(
            store
                .list_prefix(&ObjectKeyPrefix::new("chunks").expect("prefix"))
                .await
                .expect("list"),
            vec![object.clone()]
        );

        store.delete(&object).await.expect("delete");
        store
            .delete(&object)
            .await
            .expect("delete remains idempotent");
        assert!(!store.exists(&object).await.expect("exists"));
    }

    #[tokio::test]
    async fn local_store_put_if_absent_is_idempotent_for_same_bytes() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = LocalStore::new(temp.path());
        let object = key("indexes/current");

        assert_eq!(
            store.put_if_absent(&object, b"index").await.expect("put"),
            PutStatus::Created
        );
        assert_eq!(
            store
                .put_if_absent(&object, b"index")
                .await
                .expect("put again"),
            PutStatus::AlreadyPresent
        );
        assert_eq!(store.get(&object).await.expect("get"), b"index");
    }

    #[tokio::test]
    async fn local_store_rejects_conflicting_immutable_write() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = LocalStore::new(temp.path());
        let object = key("manifests/snap");

        store.put_if_absent(&object, b"first").await.expect("put");
        let error = store
            .put_if_absent(&object, b"second")
            .await
            .expect_err("conflict");
        assert!(matches!(error, StorageError::ObjectAlreadyExists { .. }));
        assert_eq!(store.get(&object).await.expect("get"), b"first");
    }

    #[tokio::test]
    async fn local_store_ignores_leftover_temporary_objects() {
        let temp = tempfile::tempdir().expect("temp dir");
        let store = LocalStore::new(temp.path());
        let object = key("chunks/bb/blob");
        let temp_dir = store.temp_dir();

        fs::create_dir_all(&temp_dir).await.expect("temp dir");
        fs::write(temp_dir.join("interrupted.part"), b"partial")
            .await
            .expect("write temp");
        store
            .put_if_absent(&object, b"complete")
            .await
            .expect("put");

        assert_eq!(
            store
                .list_prefix(&ObjectKeyPrefix::root())
                .await
                .expect("list"),
            vec![object]
        );
    }

    #[tokio::test]
    async fn s3_store_round_trip_when_integration_env_is_enabled() {
        let Some(config) = s3_integration_config() else {
            return;
        };
        let store = S3Store::new(config).expect("s3 store");
        let first = key("chunks/aa/blob");
        let second = key("indexes/current");

        assert_eq!(
            store.capabilities(),
            StorageCapabilities::s3_compatible(false)
        );
        assert!(!store.exists(&first).await.expect("exists before put"));
        assert_eq!(
            store
                .put_if_absent(&first, b"sealed-cloud")
                .await
                .expect("put first"),
            PutStatus::Created
        );
        assert_eq!(
            store
                .put_if_absent(&first, b"sealed-cloud")
                .await
                .expect("idempotent put"),
            PutStatus::AlreadyPresent
        );
        assert_eq!(
            store
                .put_if_absent(&second, b"index")
                .await
                .expect("put second"),
            PutStatus::Created
        );

        let conflict = store
            .put_if_absent(&first, b"different")
            .await
            .expect_err("conflicting put");
        assert!(matches!(conflict, StorageError::ObjectAlreadyExists { .. }));
        assert_eq!(store.get(&first).await.expect("get"), b"sealed-cloud");

        let listed = store
            .list_prefix(&ObjectKeyPrefix::new("chunks").expect("prefix"))
            .await
            .expect("list chunks");
        assert_eq!(listed, vec![first.clone()]);

        store.delete(&first).await.expect("delete first");
        store.delete(&second).await.expect("delete second");
        store.delete(&first).await.expect("idempotent delete");
        assert!(!store.exists(&first).await.expect("exists after delete"));
    }

    fn s3_integration_config() -> Option<S3StoreConfig> {
        if std::env::var("SEALPORT_S3_INTEGRATION").ok().as_deref() != Some("1") {
            return None;
        }

        let configured_prefix = required_env("SEALPORT_S3_TEST_PREFIX");
        let unique_prefix = format!("{configured_prefix}/run-{}", unique_test_id());
        let root_prefix = ObjectKeyPrefix::new(unique_prefix).expect("valid s3 test prefix");

        Some(
            S3StoreConfig::new(
                required_env("SEALPORT_S3_BUCKET"),
                required_env("SEALPORT_S3_REGION"),
                required_env("SEALPORT_S3_ENDPOINT"),
                required_env("SEALPORT_S3_ACCESS_KEY_ID"),
                required_env("SEALPORT_S3_SECRET_ACCESS_KEY"),
                root_prefix,
            )
            .expect("s3 config")
            .with_conditional_create(false),
        )
    }

    fn required_env(name: &str) -> String {
        std::env::var(name)
            .unwrap_or_else(|_| panic!("{name} must be set for S3 integration tests"))
    }

    fn unique_test_id() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        format!("{}-{nanos}", std::process::id())
    }
}
