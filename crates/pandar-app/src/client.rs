use anyhow::{Context, Result, bail};
use clap::Args;
use reqwest::Method;
use std::{path::PathBuf, time::Duration};

use crate::cli_auth::{
    build_http_client,
    config::{
        CliProfile, load_config, normalize_base_url, resolve_config_path, validate_profile_name,
    },
    fetch_whoami,
    secret::SecretString,
};

pub(crate) mod commands;
pub(crate) mod request;
mod resources;

use commands::{ClientCommand, CommandsCommand, JobsCommand, RequestArgs};
use request::{
    RawRequestParams, execute_and_print, parse_header_arg, parse_query_arg, read_body_input,
    validate_and_resolve_path, validate_path_segment,
};
use resources::{run_agents, run_jobs, run_printers, run_tokens};

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const DEFAULT_UPLOAD_TIMEOUT_SECONDS: u64 = 300;

#[derive(Debug, Args)]
pub(crate) struct ClientArgs {
    /// Saved profile name to use
    #[arg(long, env = "PANDAR_PROFILE")]
    pub profile: Option<String>,

    /// Override CLI credentials config file path
    #[arg(long, env = "PANDAR_CLI_CONFIG")]
    pub config: Option<PathBuf>,

    /// Pandar server URL override (Hub API URL)
    #[arg(long, env = "PANDAR_SERVER")]
    pub server: Option<String>,

    /// Explicit Pandar Hub API URL override (takes precedence over --server)
    #[arg(long, env = "PANDAR_HUB_URL")]
    pub hub_url: Option<String>,

    /// API key override (prefer PANDAR_API_KEY env var or `pandar login --api-key-stdin`)
    #[arg(long, env = "PANDAR_API_KEY")]
    pub api_key: Option<String>,

    /// Tenant ID override
    #[arg(long, env = "PANDAR_TENANT_ID")]
    pub tenant_id: Option<String>,

    /// Request timeout in seconds (default 60s; 300s for job artifact uploads)
    #[arg(long)]
    pub timeout_seconds: Option<u64>,

    #[command(subcommand)]
    pub command: ClientCommand,
}

pub(super) struct ResolvedConnection {
    pub(super) hub_url: String,
    pub(super) token: Option<SecretString>,
    pub(super) tenant_id: Option<String>,
}

pub(crate) async fn run_client(args: ClientArgs) -> Result<()> {
    let default_timeout = match &args.command {
        ClientCommand::Jobs(JobsCommand::Create(_)) => DEFAULT_UPLOAD_TIMEOUT_SECONDS,
        _ => DEFAULT_TIMEOUT_SECONDS,
    };
    let timeout = Duration::from_secs(args.timeout_seconds.unwrap_or(default_timeout).max(1));
    let http = build_http_client(timeout)?;

    let allow_no_auth = matches!(&args.command, ClientCommand::Request(req) if req.no_auth);
    let mut conn = resolve_connection(&args, !allow_no_auth)?;

    match args.command {
        ClientCommand::Whoami => {
            let token = require_token(&conn)?;
            let whoami = fetch_whoami(&http, &conn.hub_url, token).await?;
            println!("{}", serde_json::to_string_pretty(&whoami)?);
            Ok(())
        }
        ClientCommand::Request(req) => run_raw_request(&http, &mut conn, req).await,
        ClientCommand::Printers(cmd) => run_printers(&http, &mut conn, cmd).await,
        ClientCommand::Jobs(cmd) => run_jobs(&http, &mut conn, cmd).await,
        ClientCommand::Agents(cmd) => run_agents(&http, &mut conn, cmd).await,
        ClientCommand::Tokens(cmd) => run_tokens(&http, &mut conn, cmd).await,
        ClientCommand::Commands(CommandsCommand::Get { command_id }) => {
            validate_path_segment(&command_id, "command_id")?;
            let tenant_id = ensure_tenant_id(&http, &mut conn).await?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/commands/{command_id}",
                conn.hub_url
            );
            send_simple(&http, Method::GET, &url, require_token(&conn)?, None).await
        }
    }
}

async fn run_raw_request(
    http: &reqwest::Client,
    conn: &mut ResolvedConnection,
    req: RequestArgs,
) -> Result<()> {
    let method = reqwest::Method::from_bytes(req.method.trim().to_ascii_uppercase().as_bytes())
        .with_context(|| format!("invalid HTTP method `{}`", req.method))?;
    let trimmed = req.path.trim_start_matches('/');
    let needs_tenant = !trimmed.starts_with("api/");
    let tenant_id = if needs_tenant {
        Some(ensure_tenant_id(http, conn).await?)
    } else {
        conn.tenant_id.clone()
    };
    let resolved_path = validate_and_resolve_path(&req.path, tenant_id.as_deref())?;
    let url = format!("{}{resolved_path}", conn.hub_url);

    let headers = req
        .headers
        .iter()
        .map(|raw| parse_header_arg(raw))
        .collect::<Result<Vec<_>>>()?;
    let query = req
        .query
        .iter()
        .map(|raw| parse_query_arg(raw))
        .collect::<Result<Vec<_>>>()?;
    let body = read_body_input(req.data.as_deref(), req.data_file.as_deref())?;
    let token = if req.no_auth {
        None
    } else {
        Some(require_token(conn)?)
    };

    execute_and_print(RawRequestParams {
        http,
        method,
        url: &url,
        token,
        headers: &headers,
        query: &query,
        body,
        include_headers: req.include,
    })
    .await
}

pub(super) async fn send_simple(
    http: &reqwest::Client,
    method: Method,
    url: &str,
    token: &SecretString,
    body: Option<Vec<u8>>,
) -> Result<()> {
    execute_and_print(RawRequestParams {
        http,
        method,
        url,
        token: Some(token),
        headers: &[],
        query: &[],
        body,
        include_headers: false,
    })
    .await
}

fn resolve_connection(args: &ClientArgs, require_auth: bool) -> Result<ResolvedConnection> {
    let explicit_url = args.hub_url.as_deref().or(args.server.as_deref());
    let explicit_key = args
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| SecretString::new(value.to_owned()));

    let profile: Option<CliProfile> = if args.profile.is_none()
        && explicit_url.is_some()
        && (!require_auth || explicit_key.is_some())
    {
        None
    } else {
        let config_path = resolve_config_path(args.config.as_deref())?;
        let cfg = load_config(&config_path)?;
        let selected_name = match args.profile.as_deref() {
            Some(name) => Some(validate_profile_name(name)?),
            None => cfg.active_profile.clone(),
        };
        match selected_name {
            Some(name) => match cfg.profiles.get(&name) {
                Some(profile) => Some(profile.clone()),
                None => bail!(
                    "Pandar CLI profile `{name}` was not found in {}; run `pandar login` or pass --profile",
                    config_path.display()
                ),
            },
            None => None,
        }
    };

    let hub_url = match explicit_url {
        Some(raw) => normalize_base_url(raw)?,
        None => profile
            .as_ref()
            .map(|profile| profile.hub_url.clone())
            .context(
                "no Pandar server configured; run `pandar login` or pass --server / --hub-url",
            )?,
    };
    let token = explicit_key.or_else(|| profile.as_ref().map(|profile| profile.token.clone()));
    if require_auth && token.is_none() {
        bail!("no Pandar API key or session configured; run `pandar login` or set PANDAR_API_KEY");
    }
    let tenant_id = args
        .tenant_id
        .clone()
        .or_else(|| profile.as_ref().map(|profile| profile.tenant_id.clone()));

    Ok(ResolvedConnection {
        hub_url,
        token,
        tenant_id,
    })
}

pub(super) fn require_token(conn: &ResolvedConnection) -> Result<&SecretString> {
    conn.token.as_ref().context(
        "no Pandar API key or session configured; run `pandar login` or set PANDAR_API_KEY",
    )
}

pub(super) async fn ensure_tenant_id(
    http: &reqwest::Client,
    conn: &mut ResolvedConnection,
) -> Result<String> {
    if let Some(tenant_id) = &conn.tenant_id {
        return Ok(tenant_id.clone());
    }
    let token = require_token(conn)?;
    let whoami = fetch_whoami(http, &conn.hub_url, token).await?;
    conn.tenant_id = Some(whoami.tenant_id.clone());
    Ok(whoami.tenant_id)
}
