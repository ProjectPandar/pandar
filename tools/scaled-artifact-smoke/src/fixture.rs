use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context;
use pandar_hub::{
    AppState,
    artifacts::{ArtifactStorage, S3ArtifactStorageConfig},
    cluster::{ControlPlane, ControlPlaneConfig},
    db::{Database, DatabaseConfig},
    repositories::{AuditActor, TenantTokenScope, UserRole},
    runtime::spawn_control_plane_ready,
};

use crate::{
    agent::FakeAgent,
    harness::{HarnessConfig, HarnessMode},
    storage::shared_object_storage,
};

pub const ARTIFACT_BYTES: &[u8] = b"scaled smoke 3mf bytes";

/// Two Hub replicas sharing one database, object store, and control plane.
pub struct SmokeWorld {
    pub database: Database,
    storage: Arc<dyn ArtifactStorage>,
    control_plane: ControlPlane,
    pub hub_a: AppState,
    pub hub_b: AppState,
}

impl SmokeWorld {
    pub async fn for_config(config: &HarnessConfig) -> anyhow::Result<Self> {
        match config.mode {
            HarnessMode::DryRun => {
                let root = world_root(config)?;
                let storage = shared_object_storage(root.join("objects"))?;
                Self::dry_run(&root, storage).await
            }
            HarnessMode::Live => Self::live().await,
        }
    }

    /// A SQLite and in-process control-plane world over `storage`.
    pub async fn dry_run(root: &Path, storage: Arc<dyn ArtifactStorage>) -> anyhow::Result<Self> {
        let database_url = format!("sqlite://{}", root.join("hub.sqlite").display());
        let database = Database::connect(&DatabaseConfig::from_url(database_url)?).await?;
        database.migrate().await?;
        Self::assemble(database, storage, ControlPlane::in_process()).await
    }

    async fn live() -> anyhow::Result<Self> {
        let live = crate::live::LiveConfig::from_env()?;
        let database = Database::connect(&DatabaseConfig::from_url(live.database_url)?)
            .await
            .context("connect live soak PostgreSQL database")?;
        database
            .migrate()
            .await
            .context("migrate live soak PostgreSQL database")?;
        let control_plane = ControlPlane::from_config(ControlPlaneConfig::Nats {
            url: live.nats_url,
            subject: live.nats_subject,
        })
        .await
        .context("connect live soak NATS control plane")?;
        let storage_config = S3ArtifactStorageConfig::from_env_values(
            Some(live.s3_bucket),
            Some(live.s3_region),
            Some(live.s3_endpoint),
            Some(live.s3_access_key_id),
            Some(live.s3_secret_access_key),
            Some(if live.s3_force_path_style {
                "true"
            } else {
                "false"
            }),
            None::<String>,
        )
        .context("build live soak S3 storage config from PANDAR_SOAK_* values")?;
        let storage: Arc<dyn ArtifactStorage> = Arc::new(
            storage_config
                .build()
                .await
                .context("connect live soak S3-compatible artifact storage")?,
        );
        storage
            .check_ready()
            .await
            .context("check live soak S3-compatible artifact storage readiness")?;
        Self::assemble(database, storage, control_plane).await
    }

    async fn assemble(
        database: Database,
        storage: Arc<dyn ArtifactStorage>,
        control_plane: ControlPlane,
    ) -> anyhow::Result<Self> {
        let hub_a = replica(&database, &storage, &control_plane).await?;
        let hub_b = replica(&database, &storage, &control_plane).await?;
        Ok(Self {
            database,
            storage,
            control_plane,
            hub_a,
            hub_b,
        })
    }

    pub async fn agent(&self, hub: &AppState, fixture: &SmokeFixture) -> anyhow::Result<FakeAgent> {
        FakeAgent::connect(hub, &self.database, fixture).await
    }

    /// A fresh replica over the shared state, as after a Hub restart.
    pub async fn restarted_state(&self) -> anyhow::Result<AppState> {
        replica(&self.database, &self.storage, &self.control_plane).await
    }
}

/// A fresh directory for one dry-run world inside the run-wide scratch dir.
pub fn world_root(config: &HarnessConfig) -> anyhow::Result<PathBuf> {
    Ok(tempfile::tempdir_in(&config.scratch)
        .context("create smoke world dir")?
        .keep())
}

/// Builds a Hub replica and starts its control-plane subscription, which
/// delivers agent wakes and printer events published by any replica.
async fn replica(
    database: &Database,
    storage: &Arc<dyn ArtifactStorage>,
    control_plane: &ControlPlane,
) -> anyhow::Result<AppState> {
    let state = AppState::from_database_with_control_plane(
        database.clone(),
        storage.clone(),
        control_plane.clone(),
    )
    .await?;
    let (_, ready) = spawn_control_plane_ready(state.clone());
    ready
        .await
        .context("control plane readiness channel closed")?
        .context("hub control plane failed to start")?;
    Ok(state)
}

pub async fn seed_fixture(state: &AppState, suffix: &str) -> anyhow::Result<SmokeFixture> {
    let tenant = state
        .tenants()
        .create(
            &format!("scaled-smoke-{suffix}"),
            &format!("Scaled Smoke {suffix}"),
        )
        .await?;
    let admin = state
        .auth()
        .create_user(
            tenant.id,
            &format!("scaled-smoke-{suffix}@example.invalid"),
            "Scaled Smoke",
            UserRole::TenantAdmin,
        )
        .await?;
    let agent = state
        .agents()
        .create(tenant.id, &format!("scaled-smoke-agent-{suffix}"))
        .await?;
    let agent_credential = format!("pandar_agent_scaled_smoke_secret_{suffix}");
    state
        .agents()
        .rotate_credential(
            tenant.id,
            agent.id,
            &agent_credential,
            AuditActor::user(admin.id.clone()),
        )
        .await?;
    let plugin_token = state
        .auth()
        .create_tenant_token_with_audit(
            tenant.id,
            &format!("scaled smoke plugin {suffix}"),
            vec![TenantTokenScope::PluginStudio],
            None,
            AuditActor::user(admin.id.clone()),
        )
        .await?;
    let tenant_token = state
        .auth()
        .create_tenant_token_with_audit(
            tenant.id,
            &format!("scaled smoke tenant {suffix}"),
            vec![TenantTokenScope::All],
            None,
            AuditActor::user(admin.id),
        )
        .await?;

    Ok(SmokeFixture {
        tenant_id: tenant.id,
        agent_id: agent.id,
        printer_serial: format!("scaled-smoke-serial-{suffix}"),
        plugin_token: plugin_token.plaintext_token,
        tenant_token: tenant_token.plaintext_token,
        agent_credential,
    })
}

pub struct SmokeFixture {
    pub tenant_id: pandar_core::TenantId,
    pub agent_id: pandar_core::AgentId,
    pub printer_serial: String,
    pub plugin_token: String,
    pub tenant_token: String,
    pub agent_credential: String,
}
