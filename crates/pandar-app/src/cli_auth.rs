use anyhow::{Context, Result, bail};
use clap::Args;
use serde::{Deserialize, Serialize};
use std::{io, path::PathBuf, time::Duration};

pub(crate) mod browser;
pub(crate) mod config;
pub(crate) mod discovery;
pub(crate) mod lock;
pub(crate) mod secret;

use browser::{BrowserLoginSession, DEFAULT_BROWSER_LOGIN_TIMEOUT, try_open_browser};
use config::{
    CliAuthKind, CliProfile, load_config, normalize_base_url, resolve_config_path, save_config,
    validate_profile_name,
};
use discovery::{resolve_api_key_urls, resolve_browser_hub_url};
use lock::ConfigFileLock;
use secret::{SecretString, read_api_key_from_reader};

#[derive(Debug, Args)]
pub(crate) struct LoginArgs {
    /// Pandar server URL (Hub URL for API key login; Web URL for browser login)
    #[arg(long, env = "PANDAR_SERVER")]
    pub server: String,

    /// Explicit Pandar Hub API URL (required when Web and Hub are on different origins)
    #[arg(long, env = "PANDAR_HUB_URL")]
    pub hub_url: Option<String>,

    /// API key for login (prefer --api-key-stdin or PANDAR_API_KEY to avoid shell history/process list exposure)
    #[arg(long, env = "PANDAR_API_KEY", conflicts_with = "api_key_stdin")]
    pub api_key: Option<String>,

    /// Read the API key from stdin (recommended for automation and shared hosts)
    #[arg(long)]
    pub api_key_stdin: bool,

    /// Optional tenant ID hint for browser sign-in
    #[arg(long, env = "PANDAR_TENANT_ID")]
    pub tenant_id: Option<String>,

    /// Local profile name to save and activate
    #[arg(long, default_value = "default", env = "PANDAR_PROFILE")]
    pub profile: String,

    /// Do not automatically open the browser during browser sign-in
    #[arg(long)]
    pub no_browser: bool,

    /// Override CLI credentials config file path
    #[arg(long, env = "PANDAR_CLI_CONFIG")]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub(crate) struct LogoutArgs {
    /// Profile name to remove (defaults to active profile)
    #[arg(long, env = "PANDAR_PROFILE")]
    pub profile: Option<String>,

    /// Skip server-side session revocation when logging out a browser CLI session
    #[arg(long)]
    pub local_only: bool,

    /// Override CLI credentials config file path
    #[arg(long, env = "PANDAR_CLI_CONFIG")]
    pub config: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct WhoamiUser {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct WhoamiResponse {
    pub token_id: String,
    pub token_name: String,
    pub tenant_id: String,
    pub tenant_slug: String,
    pub tenant_display_name: String,
    pub scopes: Vec<String>,
    pub effective_role: Option<String>,
    pub expires_at: Option<String>,
    pub user: Option<WhoamiUser>,
}

#[derive(Debug, Serialize)]
pub(crate) struct LoginOutput {
    pub profile: String,
    pub hub_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_url: Option<String>,
    pub tenant_id: String,
    pub tenant_slug: String,
    pub tenant_display_name: String,
    pub auth_kind: CliAuthKind,
    pub scopes: Vec<String>,
    pub effective_role: Option<String>,
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
    pub config_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub(crate) struct LogoutOutput {
    pub profile: String,
    pub removed: bool,
    pub revoked_remote_session: bool,
    pub config_path: PathBuf,
}

pub(crate) fn build_http_client(timeout: Duration) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .build()
        .context("failed to build HTTP client")
}

pub(crate) async fn run_login(args: LoginArgs) -> Result<LoginOutput> {
    let profile_name = validate_profile_name(&args.profile)?;
    let config_path = resolve_config_path(args.config.as_deref())?;
    let server_url = normalize_base_url(&args.server)?;
    let explicit_hub_url = args
        .hub_url
        .as_deref()
        .map(normalize_base_url)
        .transpose()?;
    let http = build_http_client(Duration::from_secs(60))?;

    let api_key = if args.api_key_stdin {
        Some(read_api_key_from_reader(io::stdin().lock())?)
    } else if let Some(raw) = args.api_key {
        let trimmed = raw.trim().to_owned();
        if trimmed.is_empty() {
            bail!("--api-key must not be empty");
        }
        if trimmed.chars().any(char::is_whitespace) {
            bail!("--api-key must not contain whitespace");
        }
        Some(SecretString::new(trimmed))
    } else {
        None
    };

    let (hub_url, web_url, token, auth_kind, browser_user_name) = if let Some(key) = api_key {
        let (hub_url, web_url) =
            resolve_api_key_urls(&http, &server_url, explicit_hub_url.as_deref()).await?;
        (hub_url, web_url, key, CliAuthKind::ApiKey, None)
    } else {
        let hub_url =
            resolve_browser_hub_url(&http, &server_url, explicit_hub_url.as_deref()).await?;
        let session = BrowserLoginSession::start(&server_url, args.tenant_id.as_deref()).await?;
        eprintln!(
            "Open this URL in your browser to sign in to Pandar CLI:\n{}\n(Waiting on {})",
            session.authorize_url, session.callback_url
        );
        if !args.no_browser
            && let Err(err) = try_open_browser(&session.authorize_url)
        {
            eprintln!(
                "Could not open browser automatically ({err:#}); open the URL above manually."
            );
        }
        let exchanged = session
            .wait_and_exchange(&http, &hub_url, DEFAULT_BROWSER_LOGIN_TIMEOUT)
            .await?;
        (
            hub_url,
            Some(server_url),
            exchanged.token,
            CliAuthKind::BrowserSession,
            Some(exchanged.profile.user_name),
        )
    };

    let whoami = fetch_whoami(&http, &hub_url, &token).await?;
    let user_name = whoami
        .user
        .as_ref()
        .map(|user| user.display_name.clone())
        .or(browser_user_name);

    let _lock = ConfigFileLock::acquire(&config_path)?;
    let mut cfg = load_config(&config_path)?;
    cfg.profiles.insert(
        profile_name.clone(),
        CliProfile {
            hub_url: hub_url.clone(),
            web_url: web_url.clone(),
            tenant_id: whoami.tenant_id.clone(),
            tenant_slug: whoami.tenant_slug.clone(),
            tenant_display_name: whoami.tenant_display_name.clone(),
            token,
            auth_kind,
            token_id: Some(whoami.token_id),
            token_name: Some(whoami.token_name),
            effective_role: whoami.effective_role.clone(),
            expires_at: whoami.expires_at.clone(),
            user_name: user_name.clone(),
        },
    );
    cfg.active_profile = Some(profile_name.clone());
    save_config(&config_path, &cfg)?;

    Ok(LoginOutput {
        profile: profile_name,
        hub_url,
        web_url,
        tenant_id: whoami.tenant_id,
        tenant_slug: whoami.tenant_slug,
        tenant_display_name: whoami.tenant_display_name,
        auth_kind,
        scopes: whoami.scopes,
        effective_role: whoami.effective_role,
        expires_at: whoami.expires_at,
        user_name,
        config_path,
    })
}

pub(crate) async fn run_logout(args: LogoutArgs) -> Result<LogoutOutput> {
    let config_path = resolve_config_path(args.config.as_deref())?;
    let _lock = ConfigFileLock::acquire(&config_path)?;
    let mut cfg = load_config(&config_path)?;
    let profile_name = match args.profile {
        Some(name) => validate_profile_name(&name)?,
        None => cfg
            .active_profile
            .clone()
            .context("no active Pandar CLI profile; specify --profile or run `pandar login`")?,
    };
    let Some(removed) = cfg.profiles.remove(&profile_name) else {
        return Ok(LogoutOutput {
            profile: profile_name,
            removed: false,
            revoked_remote_session: false,
            config_path,
        });
    };

    let mut revoked_remote_session = false;
    if !args.local_only && removed.auth_kind == CliAuthKind::BrowserSession {
        let http = build_http_client(Duration::from_secs(30))?;
        let url = format!("{}/api/v1/cli/session", removed.hub_url);
        match http
            .delete(&url)
            .bearer_auth(removed.token.expose())
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                revoked_remote_session = true;
            }
            Ok(response) => {
                eprintln!(
                    "Warning: remote CLI session revocation returned HTTP {}",
                    response.status()
                );
            }
            Err(err) => {
                let err = anyhow::Error::new(err);
                eprintln!("Warning: could not revoke remote CLI session ({err:#})");
            }
        }
    }

    if cfg.active_profile.as_deref() == Some(profile_name.as_str()) {
        cfg.active_profile = cfg.profiles.keys().next().cloned();
    }
    save_config(&config_path, &cfg)?;

    Ok(LogoutOutput {
        profile: profile_name,
        removed: true,
        revoked_remote_session,
        config_path,
    })
}

pub(crate) async fn fetch_whoami(
    http: &reqwest::Client,
    hub_url: &str,
    token: &SecretString,
) -> Result<WhoamiResponse> {
    let url = format!("{hub_url}/api/v1/auth/whoami");
    let response = http
        .get(&url)
        .bearer_auth(token.expose())
        .send()
        .await
        .with_context(|| format!("failed to verify credentials against {url}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read whoami response body")?;
    if !status.is_success() {
        bail!("credential verification failed at {url} with HTTP {status}: {body}");
    }
    serde_json::from_str::<WhoamiResponse>(&body)
        .context("failed to parse whoami response from Pandar Hub")
}
