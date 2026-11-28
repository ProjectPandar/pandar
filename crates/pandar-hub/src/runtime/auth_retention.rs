use std::time::Duration;

use anyhow::Context;
use tokio::task::JoinHandle;

use crate::AppState;

const AUTH_RETENTION_CLEANUP_INTERVAL: Duration = Duration::from_secs(3600);

pub fn spawn_auth_retention_cleanup(state: AppState, retention_days: i64) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(AUTH_RETENTION_CLEANUP_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let now = pandar_core::created_at_now();
            match purge_expired_or_revoked_tenant_tokens_once(&state, &now, retention_days).await {
                Ok(0) => {}
                Ok(deleted) => {
                    tracing::info!(
                        deleted,
                        retention_days,
                        "purged expired or revoked tenant tokens"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        error = %crate::redaction::redact_secrets(&format!("{err:#}")),
                        "failed to purge expired or revoked tenant tokens"
                    );
                }
            }
            match purge_expired_or_revoked_join_links_once(&state, &now, retention_days).await {
                Ok(0) => {}
                Ok(deleted) => {
                    tracing::info!(
                        deleted,
                        retention_days,
                        "purged expired or revoked join links"
                    );
                }
                Err(err) => {
                    tracing::error!(
                        error = %crate::redaction::redact_secrets(&format!("{err:#}")),
                        "failed to purge expired or revoked join links"
                    );
                }
            }
        }
    })
}

fn retention_cutoff(now: &str, retention_days: i64) -> anyhow::Result<String> {
    let parsed = time::OffsetDateTime::parse(now, &time::format_description::well_known::Rfc3339)
        .context("failed to parse auth retention cleanup timestamp")?;
    (parsed - time::Duration::days(retention_days))
        .format(&time::format_description::well_known::Rfc3339)
        .context("failed to format auth retention cleanup cutoff")
}

async fn purge_expired_or_revoked_tenant_tokens_once(
    state: &AppState,
    now: &str,
    retention_days: i64,
) -> anyhow::Result<u64> {
    let cutoff = retention_cutoff(now, retention_days)?;
    state
        .auth()
        .purge_expired_or_revoked_tenant_tokens(&cutoff)
        .await
        .context("failed to purge expired or revoked tenant tokens")
}

async fn purge_expired_or_revoked_join_links_once(
    state: &AppState,
    now: &str,
    retention_days: i64,
) -> anyhow::Result<u64> {
    let cutoff = retention_cutoff(now, retention_days)?;
    state
        .auth()
        .purge_expired_or_revoked_join_links(&cutoff)
        .await
        .context("failed to purge expired or revoked join links")
}
