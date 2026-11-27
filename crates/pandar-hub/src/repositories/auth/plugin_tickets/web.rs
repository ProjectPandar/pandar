use anyhow::Context;
use axum::http::HeaderValue;
use pandar_core::{TenantId, created_at_now};
use sea_orm::TransactionTrait;

use super::{
    LoginTicketKind, PluginLoginTicket, PluginLoginTicketExchange, PluginLoginTicketWithPlaintext,
    WEB_LOGIN_TICKET_PREFIX, insert_plugin_login_ticket, plugin_login_ticket_audit_event,
};
use crate::repositories::{
    AuditActor, AuthRepository, RepositoryError, RepositoryResult,
    audit::insert_audit_event_tx,
    auth::{hash_token, secrets::generate_secret},
};

impl AuthRepository {
    #[allow(clippy::too_many_arguments)]
    pub async fn create_web_login_ticket_with_audit(
        &self,
        tenant_id: TenantId,
        user_id: Option<String>,
        redirect_url: impl AsRef<str>,
        allowed_origins: &[HeaderValue],
        code_challenge: String,
        expires_at: String,
        actor: AuditActor,
    ) -> RepositoryResult<PluginLoginTicketWithPlaintext> {
        let redirect_url =
            self.validate_web_redirect_url(redirect_url.as_ref(), allowed_origins)?;
        let plaintext_ticket = generate_secret(WEB_LOGIN_TICKET_PREFIX);
        let ticket = PluginLoginTicket {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id,
            user_id,
            redirect_url,
            kind: LoginTicketKind::Web,
            code_challenge: Some(code_challenge),
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
            .context("failed to begin web login ticket create transaction")?;
        insert_plugin_login_ticket(&tx, &ticket, &ticket_hash).await?;
        insert_audit_event_tx(
            &tx,
            &plugin_login_ticket_audit_event(&ticket, "web_login_ticket.create", actor, None),
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit web login ticket create transaction")?;

        Ok(PluginLoginTicketWithPlaintext {
            ticket,
            plaintext_ticket,
        })
    }

    pub async fn exchange_web_login_ticket(
        &self,
        plaintext_ticket: &str,
        code_verifier: &str,
    ) -> RepositoryResult<Option<PluginLoginTicketExchange>> {
        self.exchange_login_ticket(plaintext_ticket, LoginTicketKind::Web, Some(code_verifier))
            .await
    }

    /// Accepts only http(s) callbacks whose origin is listed in `PANDAR_CORS_ALLOWED_ORIGINS`.
    pub fn validate_web_redirect_url(
        &self,
        redirect_url: impl AsRef<str>,
        allowed_origins: &[HeaderValue],
    ) -> RepositoryResult<String> {
        let redirect_url = redirect_url.as_ref();
        let url = reqwest::Url::parse(redirect_url)
            .map_err(|_| RepositoryError::InvalidPluginRedirectUrl)?;
        let origin = url.origin().ascii_serialization();
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !allowed_origins
                .iter()
                .any(|allowed| allowed.as_bytes() == origin.as_bytes())
        {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        }

        Ok(redirect_url.to_owned())
    }
}
