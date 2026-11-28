use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use pandar_core::TenantId;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ConnectionTrait};
use serde::Serialize;
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{LoginTicketKind, PluginLoginTicket};
use crate::{
    db::{UniqueConstraint, is_foreign_key_violation, is_unique_violation},
    entities::plugin_login_tickets,
    repositories::{
        AuditActor, AuditEvent, RepositoryError, RepositoryResult,
        audit::{audit_metadata, record_audit_event},
        auth::user_exists,
    },
};

#[derive(Serialize)]
struct PluginLoginTicketAuditMetadata<'a> {
    issued_tenant_token_id: Option<&'a str>,
}

pub(super) async fn insert_plugin_login_ticket<C>(
    connection: &C,
    ticket: &PluginLoginTicket,
    ticket_hash: &str,
) -> RepositoryResult<()>
where
    C: ConnectionTrait,
{
    if let Some(user_id) = &ticket.user_id {
        user_exists(
            connection,
            ticket.tenant_id,
            user_id,
            "failed to check plugin login ticket user",
        )
        .await?
        .then_some(())
        .ok_or(RepositoryError::MissingUser)?;
    }

    let result = plugin_login_ticket_model(ticket, ticket_hash)
        .insert(connection)
        .await
        .map(|_| ());
    match result {
        Ok(()) => Ok(()),
        Err(err) if is_unique_violation(&err, UniqueConstraint::PluginLoginTicketHash) => {
            Err(RepositoryError::DuplicatePluginLoginTicketHash)
        }
        Err(err) if is_foreign_key_violation(&err) => Err(RepositoryError::MissingTenant),
        Err(err) => Err(anyhow::Error::new(err)
            .context("failed to insert plugin login ticket")
            .into()),
    }
}

pub(super) fn plugin_login_ticket_from_model(
    model: plugin_login_tickets::Model,
) -> RepositoryResult<PluginLoginTicket> {
    Ok(PluginLoginTicket {
        id: model.id,
        tenant_id: TenantId::parse(&model.tenant_id).map_err(anyhow::Error::from)?,
        user_id: model.user_id,
        redirect_url: model.redirect_url,
        kind: LoginTicketKind::parse(&model.kind)?,
        code_challenge: model.code_challenge,
        created_at: model.created_at,
        expires_at: model.expires_at,
        used_at: model.used_at,
        revoked_at: model.revoked_at,
    })
}

fn plugin_login_ticket_model(
    ticket: &PluginLoginTicket,
    ticket_hash: &str,
) -> plugin_login_tickets::ActiveModel {
    plugin_login_tickets::ActiveModel {
        id: Set(ticket.id.clone()),
        tenant_id: Set(ticket.tenant_id.to_string()),
        user_id: Set(ticket.user_id.clone()),
        ticket_hash: Set(ticket_hash.to_owned()),
        redirect_url: Set(ticket.redirect_url.clone()),
        kind: Set(ticket.kind.as_str().to_owned()),
        code_challenge: Set(ticket.code_challenge.clone()),
        created_at: Set(ticket.created_at.clone()),
        expires_at: Set(ticket.expires_at.clone()),
        used_at: Set(ticket.used_at.clone()),
        revoked_at: Set(ticket.revoked_at.clone()),
    }
}

pub(super) fn pkce_matches(ticket: &PluginLoginTicket, code_verifier: &str) -> bool {
    if !(43..=128).contains(&code_verifier.len())
        || !code_verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
    {
        return false;
    }
    let actual = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    ticket.code_challenge.as_deref() == Some(actual.as_str())
}

pub(super) fn plugin_login_ticket_expired(ticket: &PluginLoginTicket) -> RepositoryResult<bool> {
    let expires_at = OffsetDateTime::parse(&ticket.expires_at, &Rfc3339).with_context(|| {
        format!(
            "failed to parse plugin login ticket expiry for {}",
            ticket.id
        )
    })?;
    Ok(expires_at <= OffsetDateTime::now_utc())
}

pub(super) fn plugin_login_ticket_audit_event(
    ticket: &PluginLoginTicket,
    action: &'static str,
    actor: AuditActor,
    tenant_token_id: Option<String>,
) -> AuditEvent {
    record_audit_event(
        ticket.tenant_id,
        actor,
        action,
        "plugin_login_ticket",
        Some(ticket.id.clone()),
        audit_metadata(PluginLoginTicketAuditMetadata {
            issued_tenant_token_id: tenant_token_id.as_deref(),
        }),
    )
}
