use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
};

use pandar_hub::artifacts::{
    ArtifactBody, ArtifactStorage, ArtifactStorageBackend, FilesystemArtifactStorage,
    StoreArtifactInput, StoredArtifact,
};

/// Object storage both smoke Hub replicas read and write.
pub fn shared_object_storage(
    root: impl Into<PathBuf>,
) -> anyhow::Result<Arc<FilesystemArtifactStorage>> {
    Ok(Arc::new(FilesystemArtifactStorage::new(
        root.into(),
        1024 * 1024,
    )?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureMode {
    None,
    Put,
    Open,
    Delete,
}

impl FailureMode {
    fn as_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Put => 1,
            Self::Open => 2,
            Self::Delete => 3,
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Put,
            2 => Self::Open,
            3 => Self::Delete,
            _ => Self::None,
        }
    }
}

#[derive(Clone)]
pub struct FailingObjectStorage {
    inner: Arc<FilesystemArtifactStorage>,
    mode: Arc<AtomicU8>,
}

impl FailingObjectStorage {
    pub fn new(inner: Arc<FilesystemArtifactStorage>) -> Self {
        Self {
            inner,
            mode: Arc::new(AtomicU8::new(FailureMode::None.as_u8())),
        }
    }

    pub fn set_mode(&self, mode: FailureMode) {
        self.mode.store(mode.as_u8(), Ordering::SeqCst);
    }

    fn mode(&self) -> FailureMode {
        FailureMode::from_u8(self.mode.load(Ordering::SeqCst))
    }
}

#[async_trait::async_trait]
impl ArtifactStorage for FailingObjectStorage {
    async fn put_artifact(&self, input: StoreArtifactInput<'_>) -> anyhow::Result<StoredArtifact> {
        if self.mode() == FailureMode::Put {
            anyhow::bail!("injected artifact put failure");
        }
        self.inner.put_artifact(input).await
    }

    async fn open_artifact(&self, storage_key: &str) -> anyhow::Result<ArtifactBody> {
        if self.mode() == FailureMode::Open {
            anyhow::bail!("injected artifact open failure");
        }
        self.inner.open_artifact(storage_key).await
    }

    async fn delete_artifact(&self, storage_key: &str) -> anyhow::Result<()> {
        if self.mode() == FailureMode::Delete {
            anyhow::bail!("injected artifact delete failure");
        }
        self.inner.delete_artifact(storage_key).await
    }

    async fn check_ready(&self) -> anyhow::Result<()> {
        self.inner.check_ready().await
    }

    fn max_artifact_bytes(&self) -> usize {
        self.inner.max_artifact_bytes()
    }

    fn backend(&self) -> ArtifactStorageBackend {
        self.inner.backend()
    }
}
