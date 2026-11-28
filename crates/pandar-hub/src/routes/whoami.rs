use axum::{Json, extract::State, http::HeaderMap};
use serde::Serialize;

use crate::{
    AppState,
    repositories::{AuthenticatedTenantToken, TenantTokenScope, UserRole},
    routes::{ApiError, auth},
};

#[derive(Debug, Serialize)]
pub(in crate::routes) struct WhoamiResponse {
    token_id: String,
    token_name: String,
    tenant_id: String,
    tenant_slug: String,
    tenant_display_name: String,
    scopes: Vec<&'static str>,
    effective_role: Option<&'static str>,
    expires_at: Option<String>,
    user: Option<WhoamiUserResponse>,
}

#[derive(Debug, Serialize)]
pub(in crate::routes) struct WhoamiUserResponse {
    id: String,
    email: String,
    display_name: String,
    role: &'static str,
}

pub(in crate::routes) async fn whoami(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<WhoamiResponse>, ApiError> {
    let authenticated = auth::authenticate_bearer_tenant_token(&state, &headers).await?;
    let tenant = state
        .tenants()
        .get(authenticated.token.tenant_id)
        .await?
        .ok_or_else(|| ApiError::not_found("tenant_not_found"))?;
    let effective_role = effective_role(&authenticated).map(UserRole::as_str);
    let user = authenticated.session_user.map(|user| WhoamiUserResponse {
        id: user.id,
        email: user.email,
        display_name: user.display_name,
        role: user.role.as_str(),
    });
    let token = authenticated.token;

    Ok(Json(WhoamiResponse {
        token_id: token.id,
        token_name: token.name,
        tenant_id: tenant.id.to_string(),
        tenant_slug: tenant.slug,
        tenant_display_name: tenant.display_name,
        scopes: token
            .scopes
            .into_iter()
            .map(TenantTokenScope::as_str)
            .collect(),
        effective_role,
        expires_at: token.expires_at,
        user,
    }))
}

fn effective_role(authenticated: &AuthenticatedTenantToken) -> Option<UserRole> {
    if authenticated.token.has_scope(TenantTokenScope::All) {
        return Some(UserRole::TenantAdmin);
    }
    if matches!(authenticated.token.scopes.as_slice(), [scope] if scope.is_user_session()) {
        return authenticated.session_user.as_ref().map(|user| user.role);
    }
    if authenticated.token.has_scope(TenantTokenScope::Operator) {
        return Some(UserRole::Operator);
    }
    if authenticated.token.has_scope(TenantTokenScope::Viewer)
        || authenticated.token.scopes.is_empty()
    {
        return Some(UserRole::Viewer);
    }
    None
}
