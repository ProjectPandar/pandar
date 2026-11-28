use anyhow::Context;
use pandar_core::{TenantId, created_at_now};

use super::{
    CLI_LOGIN_TICKET_PREFIX, LoginTicketKind, PluginLoginTicket, PluginLoginTicketExchange,
    PluginLoginTicketWithPlaintext, insert_plugin_login_ticket, plugin_login_ticket_audit_event,
};
use crate::repositories::{
    AuditActor, AuthRepository, RepositoryError, RepositoryResult,
    audit::insert_audit_event_tx,
    auth::{hash_token, secrets::generate_secret},
};

impl AuthRepository {
    pub async fn create_cli_login_ticket_with_audit(
        &self,
        tenant_id: TenantId,
        user_id: Option<String>,
        redirect_url: impl AsRef<str>,
        code_challenge: String,
        expires_at: String,
        actor: AuditActor,
    ) -> RepositoryResult<PluginLoginTicketWithPlaintext> {
        let redirect_url = self.validate_cli_redirect_url(redirect_url.as_ref())?;
        let plaintext_ticket = generate_secret(CLI_LOGIN_TICKET_PREFIX);
        let ticket = PluginLoginTicket {
            id: uuid::Uuid::new_v4().to_string(),
            tenant_id,
            user_id,
            redirect_url,
            kind: LoginTicketKind::Cli,
            code_challenge: Some(code_challenge),
            created_at: created_at_now(),
            expires_at,
            used_at: None,
            revoked_at: None,
        };
        let ticket_hash = hash_token(&plaintext_ticket);

        let tx = self
            .database
            .begin_write_transaction()
            .await
            .context("failed to begin cli login ticket create transaction")?;
        insert_plugin_login_ticket(&tx, &ticket, &ticket_hash).await?;
        insert_audit_event_tx(
            &tx,
            &plugin_login_ticket_audit_event(&ticket, "cli_login_ticket.create", actor, None),
        )
        .await?;
        tx.commit()
            .await
            .context("failed to commit cli login ticket create transaction")?;

        Ok(PluginLoginTicketWithPlaintext {
            ticket,
            plaintext_ticket,
        })
    }

    pub async fn exchange_cli_login_ticket(
        &self,
        plaintext_ticket: &str,
        code_verifier: &str,
    ) -> RepositoryResult<Option<PluginLoginTicketExchange>> {
        self.exchange_login_ticket(plaintext_ticket, LoginTicketKind::Cli, Some(code_verifier))
            .await
    }

    /// Accepts only query-free loopback http callbacks (`localhost`, `127.0.0.1`, `[::1]`)
    /// with a non-zero port and non-empty path.
    pub fn validate_cli_redirect_url(
        &self,
        redirect_url: impl AsRef<str>,
    ) -> RepositoryResult<String> {
        let redirect_url = redirect_url.as_ref();
        let url = reqwest::Url::parse(redirect_url)
            .map_err(|_| RepositoryError::InvalidPluginRedirectUrl)?;
        let Some(host) = url.host_str() else {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        };
        let Some(port) = url.port() else {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        };
        if url.scheme() != "http"
            || !url.username().is_empty()
            || url.password().is_some()
            || !matches!(host, "localhost" | "127.0.0.1" | "[::1]")
            || port == 0
            || url.path().is_empty()
            || url.query().is_some()
            || url.fragment().is_some()
            || redirect_url.contains('?')
            || redirect_url.contains('#')
        {
            return Err(RepositoryError::InvalidPluginRedirectUrl);
        }

        Ok(redirect_url.to_owned())
    }
}
