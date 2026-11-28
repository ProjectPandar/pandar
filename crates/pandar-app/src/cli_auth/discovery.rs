use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::time::Duration;

use super::config::normalize_base_url;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
struct WellKnownPandar {
    #[serde(rename = "hubUrl")]
    hub_url: Option<serde_json::Value>,
}

pub(crate) async fn resolve_api_key_urls(
    http: &reqwest::Client,
    server_url: &str,
    explicit_hub_url: Option<&str>,
) -> Result<(String, Option<String>)> {
    if let Some(hub_url) = explicit_hub_url {
        return Ok((hub_url.to_owned(), Some(server_url.to_owned())));
    }
    let well_known_url = format!("{server_url}/.well-known/pandar");
    let response = http
        .get(&well_known_url)
        .timeout(DISCOVERY_TIMEOUT)
        .send()
        .await
        .with_context(|| {
            format!(
                "failed to probe discovery endpoint {well_known_url}; pass --hub-url explicitly if needed"
            )
        })?;
    match response.status() {
        reqwest::StatusCode::OK => {
            let hub_url = parse_well_known_hub_url(response, &well_known_url).await?;
            if !same_origin(server_url, &hub_url)? {
                bail!(
                    "refusing to send API key to cross-origin hubUrl `{hub_url}` discovered from `{server_url}`; pass `--hub-url {hub_url}` explicitly or pass the Hub URL as `--server`"
                );
            }
            Ok((hub_url, Some(server_url.to_owned())))
        }
        status if is_missing_discovery_status(status) => Ok((server_url.to_owned(), None)),
        status => bail!(
            "discovery endpoint {well_known_url} returned unexpected HTTP {status}; refusing to fall back to {server_url} (pass --hub-url explicitly if intended)"
        ),
    }
}

pub(crate) async fn resolve_browser_hub_url(
    http: &reqwest::Client,
    web_url: &str,
    explicit_hub_url: Option<&str>,
) -> Result<String> {
    if let Some(hub_url) = explicit_hub_url {
        return Ok(hub_url.to_owned());
    }
    let well_known_url = format!("{web_url}/.well-known/pandar");
    let response = http
        .get(&well_known_url)
        .timeout(DISCOVERY_TIMEOUT)
        .send()
        .await
        .with_context(|| {
            format!("failed to discover Hub URL from {well_known_url}; pass --hub-url explicitly")
        })?;
    if response.status() != reqwest::StatusCode::OK {
        bail!(
            "failed to discover Hub URL from {well_known_url} (HTTP {}); pass --hub-url explicitly",
            response.status()
        );
    }
    let hub_url = parse_well_known_hub_url(response, &well_known_url).await?;
    let parsed_web = reqwest::Url::parse(web_url)?;
    let parsed_hub = reqwest::Url::parse(&hub_url)?;
    if parsed_web.scheme() == "https" && parsed_hub.scheme() != "https" {
        bail!(
            "refusing insecure http hubUrl `{hub_url}` discovered from https server `{web_url}`; pass --hub-url explicitly to override"
        );
    }
    let same_host = parsed_web.host_str() == parsed_hub.host_str();
    let both_loopback =
        is_loopback_host(parsed_web.host_str()) && is_loopback_host(parsed_hub.host_str());
    if !same_host && !both_loopback {
        bail!(
            "refusing cross-host hubUrl `{hub_url}` discovered from `{web_url}`; pass `--hub-url {hub_url}` explicitly"
        );
    }
    Ok(hub_url)
}

fn is_missing_discovery_status(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::METHOD_NOT_ALLOWED
            | reqwest::StatusCode::GONE
    )
}

async fn parse_well_known_hub_url(response: reqwest::Response, source_url: &str) -> Result<String> {
    let body = response
        .text()
        .await
        .with_context(|| format!("failed to read discovery document from {source_url}"))?;
    let payload: WellKnownPandar = serde_json::from_str(&body)
        .with_context(|| format!("invalid JSON in discovery document from {source_url}"))?;
    let Some(raw_hub_url) = payload.hub_url.as_ref().and_then(serde_json::Value::as_str) else {
        bail!("discovery document at {source_url} is missing a valid string `hubUrl`");
    };
    normalize_base_url(raw_hub_url)
        .with_context(|| format!("invalid `hubUrl` in discovery document at {source_url}"))
}

fn same_origin(left: &str, right: &str) -> Result<bool> {
    let left = reqwest::Url::parse(left)?;
    let right = reqwest::Url::parse(right)?;
    Ok(left.scheme() == right.scheme()
        && left.host_str() == right.host_str()
        && left.port_or_known_default() == right.port_or_known_default())
}

fn is_loopback_host(host: Option<&str>) -> bool {
    matches!(host, Some("localhost" | "127.0.0.1" | "[::1]"))
}
