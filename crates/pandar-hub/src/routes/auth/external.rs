use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};

use crate::{
    AppState,
    identity::VerifiedExternalIdentity,
    repositories::{AuthenticatedTenantToken, ExternalIdentityProfile},
    routes::ApiError,
};

pub(in crate::routes) fn bearer_token(headers: &HeaderMap) -> Result<&str, ApiError> {
    let Some(header) = headers.get(AUTHORIZATION) else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "missing_auth_token",
        ));
    };
    let header = header
        .to_str()
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "invalid_auth_token"))?;
    let Some(token) = header.strip_prefix("Bearer ") else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_auth_token",
        ));
    };
    Ok(token)
}

/// Strictly authenticates a Bearer tenant token (API key or session token),
/// deliberately bypassing `no_auth_enabled()` and never accepting external JWTs.
pub(in crate::routes) async fn authenticate_bearer_tenant_token(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedTenantToken, ApiError> {
    let token = bearer_token(headers)?;
    state
        .auth()
        .authenticate_tenant_token(token)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "invalid_auth_token"))
}

pub(in crate::routes) async fn verify_external_identity(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<VerifiedExternalIdentity, ApiError> {
    let token = bearer_token(headers)?;
    let Some(verifier) = state.external_auth() else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid_auth_token",
        ));
    };
    verifier.verify(token).await.map_err(|err| {
        let error = anyhow::Error::from(err);
        tracing::debug!(
            error = %format!("{error:#}"),
            "external bearer token verification failed"
        );
        ApiError::new(StatusCode::UNAUTHORIZED, "invalid_auth_token")
    })
}

pub(in crate::routes) fn external_profile(
    verified: &VerifiedExternalIdentity,
) -> Result<ExternalIdentityProfile, ApiError> {
    let Some(email) = verified.verified_email() else {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "external_email_unverified",
        ));
    };
    Ok(ExternalIdentityProfile {
        provider: verified.provider.clone(),
        subject: verified.subject.clone(),
        email: email.to_owned(),
        display_name: verified.display_name(),
    })
}
