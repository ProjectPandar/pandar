use axum::{
    Json,
    extract::rejection::JsonRejection,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

use super::{
    CreateLoginTicketRequest, ExchangeLoginTicketRequest, ExchangeLoginTicketResponse,
    LoginTicketResponse, PluginProfileResponse, plugin_login_ticket_expires_at,
    plugin_ticket_error, user_id, valid_pkce_challenge,
};
use crate::{
    AppState,
    routes::{ApiError, auth},
};

pub(in crate::routes) async fn create_web_login_ticket(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tenant_id): Path<String>,
    payload: Result<Json<CreateLoginTicketRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<LoginTicketResponse>), ApiError> {
    let tenant_id = crate::routes::parse_tenant_id(&tenant_id)?;
    let principal =
        auth::authorize_session_login_ticket_creation(&state, &headers, tenant_id).await?;
    let Json(payload) = payload.map_err(|_| ApiError::bad_request("bad_request"))?;
    let code_challenge = payload
        .code_challenge
        .filter(|value| valid_pkce_challenge(value))
        .ok_or_else(|| ApiError::bad_request("invalid_code_challenge"))?;
    let created = state
        .auth()
        .create_web_login_ticket_with_audit(
            tenant_id,
            user_id(&principal),
            payload.redirect_url,
            state.cors_allowed_origins(),
            code_challenge,
            plugin_login_ticket_expires_at()?,
            auth::audit_actor(&principal),
        )
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(LoginTicketResponse {
            ticket: created.plaintext_ticket,
            expires_at: created.ticket.expires_at,
            redirect_url: created.ticket.redirect_url,
        }),
    ))
}

pub(in crate::routes) async fn exchange_web_login_ticket(
    State(state): State<AppState>,
    payload: Result<Json<ExchangeLoginTicketRequest>, JsonRejection>,
) -> Result<Json<ExchangeLoginTicketResponse>, ApiError> {
    let Json(payload) = payload.map_err(|_| ApiError::bad_request("bad_request"))?;
    let exchanged = state
        .auth()
        .exchange_web_login_ticket(
            &payload.ticket,
            payload.code_verifier.as_deref().unwrap_or_default(),
        )
        .await
        .map_err(plugin_ticket_error)?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "invalid_plugin_ticket"))?;
    let token = exchanged.tenant_token.token;
    let tenant = state
        .tenants()
        .get(token.tenant_id)
        .await?
        .ok_or_else(|| ApiError::not_found("tenant_not_found"))?;
    let profile = PluginProfileResponse {
        user_id: token
            .created_by_user_id
            .clone()
            .unwrap_or_else(|| token.id.clone()),
        user_name: token.name.clone(),
        tenant_id: token.tenant_id.to_string(),
        tenant_name: tenant.display_name,
    };

    Ok(Json(ExchangeLoginTicketResponse {
        token: exchanged.tenant_token.plaintext_token,
        expires_at: token.expires_at.expect("web token must have expiry"),
        profile,
    }))
}
