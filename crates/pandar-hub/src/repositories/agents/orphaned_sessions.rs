use anyhow::Context;
use pandar_core::{Agent, AgentId, AgentStatus, TenantId};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{entities::agents, repositories::RepositoryResult};

use super::AgentRepository;

impl AgentRepository {
    /// Retires persisted online sessions that no hub instance holds in
    /// memory. `SessionRegistry::expire_stale` only sees this instance's
    /// sessions, so an Agent that was online when its instance restarted or
    /// crashed stays `online` until its persisted heartbeat is compared with
    /// the same stale cutoff.
    pub async fn expire_orphaned_online_sessions(
        &self,
        now: &str,
        cutoff: OffsetDateTime,
    ) -> RepositoryResult<Vec<Agent>> {
        let online = agents::Entity::find()
            .filter(agents::Column::Status.eq(AgentStatus::Online.as_str()))
            .filter(agents::Column::CurrentSessionId.is_not_null())
            .all(&self.database.sea_orm_connection())
            .await
            .context("failed to list online agents for orphaned session expiry")?;

        let mut expired = Vec::new();
        for model in online {
            let last_seen_at = model
                .last_seen_at
                .as_deref()
                .map(|value| OffsetDateTime::parse(value, &Rfc3339))
                .transpose()
                .context("failed to parse agent heartbeat during orphaned session expiry")?;
            if last_seen_at.is_some_and(|last_seen_at| last_seen_at > cutoff) {
                continue;
            }
            let tenant_id = TenantId::parse(&model.tenant_id).map_err(anyhow::Error::from)?;
            let agent_id = AgentId::parse(&model.id).map_err(anyhow::Error::from)?;
            let session_id = model
                .current_session_id
                .expect("filtered on current_session_id");
            if let Some(agent) = self
                .mark_offline_if_current(tenant_id, agent_id, &session_id, now)
                .await?
            {
                expired.push(agent);
            }
        }

        Ok(expired)
    }
}
