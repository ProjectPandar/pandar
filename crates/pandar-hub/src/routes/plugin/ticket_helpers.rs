use axum::http::StatusCode;
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    repositories::{AuthenticatedPrincipal, RepositoryError},
    routes::ApiError,
};

pub(super) fn user_id(principal: &AuthenticatedPrincipal) -> Option<String> {
    match principal {
        AuthenticatedPrincipal::User(authenticated) => Some(authenticated.user.id.clone()),
        AuthenticatedPrincipal::TenantToken(authenticated) => {
            authenticated.token.created_by_user_id.clone()
        }
        AuthenticatedPrincipal::NoAuth { .. } => None,
    }
}

pub(super) fn valid_pkce_challenge(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(super) fn plugin_ticket_error(err: RepositoryError) -> ApiError {
    match err {
        RepositoryError::MissingPluginLoginTicket => {
            ApiError::new(StatusCode::UNAUTHORIZED, "invalid_plugin_ticket")
        }
        other => other.into(),
    }
}

pub(super) fn plugin_login_ticket_expires_at() -> Result<String, ApiError> {
    (OffsetDateTime::now_utc() + Duration::minutes(5))
        .format(&Rfc3339)
        .map_err(|err| {
            tracing::error!(error = %format!("{err:#}"), "failed to format plugin login ticket expiry");
            ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_server_error")
        })
}
