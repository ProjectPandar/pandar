use anyhow::Context;
use axum::http::{HeaderValue, uri::Authority};

use crate::camera_sessions::DEFAULT_MAX_CAMERA_STREAMS_PER_TENANT;

pub(crate) fn tenant_self_create_allowed_from_env() -> anyhow::Result<bool> {
    env_bool("PANDAR_AUTH_ALLOW_TENANT_SELF_CREATE", true)
}

pub(crate) fn no_auth_from_env() -> anyhow::Result<bool> {
    env_bool("PANDAR_HUB_NO_AUTH", false)
}

pub(crate) fn camera_max_streams_per_tenant_from_env() -> anyhow::Result<usize> {
    let name = "PANDAR_HUB_CAMERA_MAX_STREAMS_PER_TENANT";
    let value = std::env::var(name).ok();
    parse_camera_max_streams_per_tenant(name, value.as_deref())
}

fn parse_camera_max_streams_per_tenant(name: &str, value: Option<&str>) -> anyhow::Result<usize> {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
        return Ok(DEFAULT_MAX_CAMERA_STREAMS_PER_TENANT);
    };
    let value = value
        .trim()
        .parse::<usize>()
        .with_context(|| format!("{name} must be a positive integer"))?;
    if value == 0 {
        anyhow::bail!("{name} must be a positive integer");
    }
    Ok(value)
}

fn env_bool(name: &str, default: bool) -> anyhow::Result<bool> {
    let Some(value) = std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(default);
    };
    match value.trim() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => anyhow::bail!("{name} must be true or false"),
    }
}

const CORS_ALLOWED_ORIGINS: &str = "PANDAR_CORS_ALLOWED_ORIGINS";

pub(crate) fn cors_allowed_origins_from_env() -> anyhow::Result<Vec<HeaderValue>> {
    let value = std::env::var(CORS_ALLOWED_ORIGINS).ok();
    parse_cors_allowed_origins(value.as_deref().unwrap_or_default())
}

pub(crate) fn parse_cors_allowed_origins(value: &str) -> anyhow::Result<Vec<HeaderValue>> {
    value
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(parse_cors_origin)
        .collect()
}

fn parse_cors_origin(origin: &str) -> anyhow::Result<HeaderValue> {
    let exact_origin = origin.split_once("://").is_some_and(|(scheme, authority)| {
        matches!(scheme, "http" | "https")
            && !authority.contains(['*', '@'])
            && authority.parse::<Authority>().is_ok()
    });
    if !exact_origin {
        anyhow::bail!(
            "{CORS_ALLOWED_ORIGINS} entry {origin:?} must be an exact http(s) origin such as https://app.example.com, without wildcard, path, or userinfo"
        );
    }
    HeaderValue::from_str(origin).with_context(|| {
        format!("{CORS_ALLOWED_ORIGINS} entry {origin:?} is not a valid header value")
    })
}
