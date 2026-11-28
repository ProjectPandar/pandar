use anyhow::Context;
use axum::http::Uri;
use pandar_core::{TenantId, created_at_now};
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

mod cli;
mod helpers;
mod mobile;
mod types;
mod web;

pub use types::{
    LoginTicketKind, PluginLoginTicket, PluginLoginTicketExchange, PluginLoginTicketWithPlaintext,
};

use crate::{
    entities::plugin_login_tickets,
    repositories::{
        AuditActor, AuthRepository, RepositoryError, RepositoryResult,
        audit::insert_audit_event_tx,
        auth::{hash_token, secrets::generate_secret},
    },
};
use helpers::{
    insert_plugin_login_ticket, pkce_matches, plugin_login_ticket_audit_event,
    plugin_login_ticket_expired, plugin_login_ticket_from_model,
};

const PLUGIN_LOGIN_TICKET_PREFIX: &str = "pandar_plugin_ticket_";
const MOBILE_LOGIN_TICKET_PREFIX: &str = "pandar_mobile_ticket_";
const WEB_LOGIN_TICKET_PREFIX: &str = "pandar_web_ticket_";
const CLI_LOGIN_TICKET_PREFIX: &str = "pandar_cli_ticket_";
const PLUGIN_TOKEN_SCOPE: &str = "plugin:studio";
const MOBILE_TOKEN_SCOPE: &str = "mobile:session";
const WEB_TOKEN_SCOPE: &str = "web:session";
const CLI_TOKEN_SCOPE: &str = "cli:session";
const PLUGIN_TOKEN_TTL_DAYS: i64 = 30;

impl AuthRepository {
    pub async fn create_plugin_login_ticket_with_audit(
        &self,
        tenant_id: TenantId,
        user_id: Option<String>,
        redirect_url: impl AsRef<str>,
        expires_at: String,
        actor: AuditActor,
    ) -> RepositoryResult<PluginLoginTicketWithPlaintext> {
        let redirect_url = self.validate_plugin_redirect_url(redirect_url.as_ref())?;
        let plaintext_ticket = generate_secret(PLUGIN_LOGIN_TICKET_PREFIX);
        let ticket = PluginLoginTicket {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id,
            user_id,
            redirect_url,
            kind: LoginTicketKind::Plugin,
            code_challenge: None,
            created_at: created_at_now(),
            expires_at,
            used_at: None,
            revoked_at: None,
        };
        let ticket_hash = hash_token(&plaintext_ticket);

        let connection = self.database.sea_orm_connection();
        let tx = connection
            .begin()
            .await
            .context("failed to begin plugin login ticket create transaction")?;
        insert_plugin_login_ticket(&tx, &ticket, &ticket_hash).await?;
        insert_audit_event_tx(
            &tx,
            &plugin_login_ticket_audit_event(&ticket, "plugin_login_ticket.create", actor, None),
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit plugin login ticket create transaction")?;

        Ok(PluginLoginTicketWithPlaintext {
            ticket,
            plaintext_ticket,
        })
    }

    pub async fn exchange_plugin_login_ticket(
        &self,
        plaintext_ticket: &str,
    ) -> RepositoryResult<Option<PluginLoginTicketExchange>> {
        self.exchange_login_ticket(plaintext_ticket, LoginTicketKind::Plugin, None)
            .await
    }

    pub async fn exchange_mobile_login_ticket(
        &self,
        plaintext_ticket: &str,
        code_verifier: &str,
    ) -> RepositoryResult<Option<PluginLoginTicketExchange>> {
        self.exchange_login_ticket(
            plaintext_ticket,
            LoginTicketKind::Mobile,
            Some(code_verifier),
        )
        .await
    }

    async fn exchange_login_ticket(
        &self,
        plaintext_ticket: &str,
        expected_kind: LoginTicketKind,
        code_verifier: Option<&str>,
    ) -> RepositoryResult<Option<PluginLoginTicketExchange>> {
        let ticket_hash = hash_token(plaintext_ticket);
        let tx = self
            .database
            .begin_write_transaction()
            .await
            .context("failed to begin plugin login ticket exchange transaction")?;

        let Some(model) = plugin_login_tickets::Entity::find()
            .filter(plugin_login_tickets::Column::TicketHash.eq(ticket_hash))
            .filter(plugin_login_tickets::Column::Kind.eq(expected_kind.as_str()))
            .filter(plugin_login_tickets::Column::UsedAt.is_null())
            .filter(plugin_login_tickets::Column::RevokedAt.is_null())
            .one(&tx)
            .await
            .context("failed to load plugin login ticket for exchange")?
        else {
            return Ok(None);
        };
        let ticket = plugin_login_ticket_from_model(model)?;
        if plugin_login_ticket_expired(&ticket)? {
            return Ok(None);
        }
        if expected_kind != LoginTicketKind::Plugin
            && !pkce_matches(&ticket, code_verifier.unwrap_or_default())
        {
            return Ok(None);
        }

        let used_at = created_at_now();
        let result = plugin_login_tickets::Entity::update_many()
            .set(plugin_login_tickets::ActiveModel {
                used_at: Set(Some(used_at.clone())),
                ..Default::default()
            })
            .filter(plugin_login_tickets::Column::Id.eq(ticket.id.clone()))
            .filter(plugin_login_tickets::Column::UsedAt.is_null())
            .filter(plugin_login_tickets::Column::RevokedAt.is_null())
            .exec(&tx)
            .await
            .context("failed to mark plugin login ticket used")?;
        if result.rows_affected != 1 {
            return Ok(None);
        }

        let mut used_ticket = ticket.clone();
        used_ticket.used_at = Some(used_at);
        let token_expires_at = (OffsetDateTime::now_utc() + Duration::days(PLUGIN_TOKEN_TTL_DAYS))
            .format(&Rfc3339)
            .context("failed to format plugin tenant token expiry")?;
        let tenant_token = match expected_kind {
            LoginTicketKind::Plugin => {
                AuthRepository::create_plugin_token_from_ticket_tx(
                    &tx,
                    used_ticket.tenant_id,
                    "Bambu Studio plugin",
                    used_ticket.user_id.clone(),
                    token_expires_at,
                )
                .await?
            }
            LoginTicketKind::Mobile => {
                AuthRepository::create_mobile_token_from_ticket_tx(
                    &tx,
                    used_ticket.tenant_id,
                    "Android app",
                    used_ticket.user_id.clone(),
                    token_expires_at,
                )
                .await?
            }
            LoginTicketKind::Web => {
                AuthRepository::create_web_token_from_ticket_tx(
                    &tx,
                    used_ticket.tenant_id,
                    "Ares web",
                    used_ticket.user_id.clone(),
                    token_expires_at,
                )
                .await?
            }
            LoginTicketKind::Cli => {
                AuthRepository::create_cli_token_from_ticket_tx(
                    &tx,
                    used_ticket.tenant_id,
                    "Pandar CLI",
                    used_ticket.user_id.clone(),
                    token_expires_at,
                )
                .await?
            }
        };
        let (event_name, actor) = match expected_kind {
            LoginTicketKind::Plugin => (
                "plugin_login_ticket.exchange",
                AuditActor::plugin_token(
                    used_ticket.user_id.clone(),
                    tenant_token.token.id.clone(),
                    vec![PLUGIN_TOKEN_SCOPE],
                ),
            ),
            LoginTicketKind::Mobile => (
                "mobile_login_ticket.exchange",
                AuditActor::tenant_token(
                    used_ticket.user_id.clone(),
                    tenant_token.token.id.clone(),
                    vec![MOBILE_TOKEN_SCOPE],
                ),
            ),
            LoginTicketKind::Web => (
                "web_login_ticket.exchange",
                AuditActor::tenant_token(
                    used_ticket.user_id.clone(),
                    tenant_token.token.id.clone(),
                    vec![WEB_TOKEN_SCOPE],
                ),
            ),
            LoginTicketKind::Cli => (
                "cli_login_ticket.exchange",
                AuditActor::tenant_token(
                    used_ticket.user_id.clone(),
                    tenant_token.token.id.clone(),
                    vec![CLI_TOKEN_SCOPE],
                ),
            ),
        };
        insert_audit_event_tx(
            &tx,
            &plugin_login_ticket_audit_event(
                &used_ticket,
                event_name,
                actor,
                Some(tenant_token.token.id.clone()),
            ),
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit plugin login ticket exchange transaction")?;

        Ok(Some(PluginLoginTicketExchange {
            redirect_url: used_ticket.redirect_url.clone(),
            ticket: used_ticket,
            tenant_token,
        }))
    }

    pub fn validate_plugin_redirect_url(
        &self,
        redirect_url: impl AsRef<str>,
    ) -> RepositoryResult<String> {
        let redirect_url = redirect_url.as_ref();
        let uri = redirect_url
            .parse::<Uri>()
            .map_err(|_| RepositoryError::InvalidPluginRedirectUrl)?;
        if uri.scheme_str() != Some("http") || redirect_url.contains('#') {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        }
        let authority = uri
            .authority()
            .ok_or(RepositoryError::InvalidPluginRedirectUrl)?;
        if authority.as_str().contains('@') {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        }
        let host = authority.host();
        if !matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]") {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        }
        let Some(port) = authority.port_u16() else {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        };
        if port == 0 {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        }

        Ok(uri.to_string())
    }
}
