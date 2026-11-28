use anyhow::{Context, Result, bail};
use reqwest::{Method, multipart};
use serde::Serialize;

use crate::cli_auth::secret::SecretString;

use super::{
    ResolvedConnection,
    commands::{AgentsCommand, CreateJobArgs, JobsCommand, PrintersCommand, TokensCommand},
    ensure_tenant_id,
    request::{render_response, validate_path_segment},
    require_token, send_simple,
};

#[derive(Serialize)]
struct ReasonBody {
    reason: Option<String>,
}

#[derive(Serialize)]
struct DiscoverBody {
    timeout_seconds: Option<u32>,
}

#[derive(Serialize)]
struct DiagnoseBody {
    serial_number: String,
}

#[derive(Serialize)]
struct CreateTokenBody {
    name: String,
    scopes: Vec<String>,
    expires_at: Option<String>,
}

#[derive(Serialize)]
struct RotateTokenBody {
    expires_at: Option<String>,
}

pub(super) async fn run_printers(
    http: &reqwest::Client,
    conn: &mut ResolvedConnection,
    cmd: PrintersCommand,
) -> Result<()> {
    let tenant_id = ensure_tenant_id(http, conn).await?;
    let token = require_token(conn)?;
    match cmd {
        PrintersCommand::List => {
            let url = format!("{}/api/v1/tenants/{tenant_id}/printers", conn.hub_url);
            send_simple(http, Method::GET, &url, token, None).await
        }
        PrintersCommand::Get { printer_id } => {
            validate_path_segment(&printer_id, "printer_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/printers/{printer_id}",
                conn.hub_url
            );
            send_simple(http, Method::GET, &url, token, None).await
        }
        PrintersCommand::Control {
            printer_id,
            action,
            payload,
        } => {
            validate_path_segment(&printer_id, "printer_id")?;
            let mut map = match payload {
                Some(raw) => {
                    serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&raw)
                        .context("--payload must be a valid JSON object")?
                }
                None => serde_json::Map::new(),
            };
            map.insert("action".to_owned(), serde_json::Value::String(action));
            let body = serde_json::to_vec(&serde_json::Value::Object(map))?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/printers/{printer_id}/controls",
                conn.hub_url
            );
            send_simple(http, Method::POST, &url, token, Some(body)).await
        }
        PrintersCommand::RefreshMaterials { printer_id } => {
            validate_path_segment(&printer_id, "printer_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/printers/{printer_id}/materials:refresh",
                conn.hub_url
            );
            send_simple(http, Method::POST, &url, token, Some(b"{}".to_vec())).await
        }
        PrintersCommand::Delete { printer_id } => {
            validate_path_segment(&printer_id, "printer_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/printers/{printer_id}",
                conn.hub_url
            );
            send_simple(http, Method::DELETE, &url, token, None).await
        }
    }
}

pub(super) async fn run_jobs(
    http: &reqwest::Client,
    conn: &mut ResolvedConnection,
    cmd: JobsCommand,
) -> Result<()> {
    let tenant_id = ensure_tenant_id(http, conn).await?;
    let token = require_token(conn)?;
    match cmd {
        JobsCommand::List => {
            let url = format!("{}/api/v1/tenants/{tenant_id}/jobs", conn.hub_url);
            send_simple(http, Method::GET, &url, token, None).await
        }
        JobsCommand::Get { job_id } => {
            validate_path_segment(&job_id, "job_id")?;
            let url = format!("{}/api/v1/tenants/{tenant_id}/jobs/{job_id}", conn.hub_url);
            send_simple(http, Method::GET, &url, token, None).await
        }
        JobsCommand::Create(create) => {
            run_create_job(http, &conn.hub_url, &tenant_id, token, create).await
        }
        JobsCommand::Retry { job_id, reason } => {
            validate_path_segment(&job_id, "job_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/jobs/{job_id}/retry-dispatch",
                conn.hub_url
            );
            let body = serde_json::to_vec(&ReasonBody { reason })?;
            send_simple(http, Method::POST, &url, token, Some(body)).await
        }
        JobsCommand::Reprint { job_id, reason } => {
            validate_path_segment(&job_id, "job_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/jobs/{job_id}/reprint",
                conn.hub_url
            );
            let body = serde_json::to_vec(&ReasonBody { reason })?;
            send_simple(http, Method::POST, &url, token, Some(body)).await
        }
        JobsCommand::Delete { job_id } => {
            validate_path_segment(&job_id, "job_id")?;
            let url = format!("{}/api/v1/tenants/{tenant_id}/jobs/{job_id}", conn.hub_url);
            send_simple(http, Method::DELETE, &url, token, None).await
        }
    }
}

async fn run_create_job(
    http: &reqwest::Client,
    hub_url: &str,
    tenant_id: &str,
    token: &SecretString,
    args: CreateJobArgs,
) -> Result<()> {
    validate_path_segment(&args.printer_id, "printer_id")?;
    let meta = tokio::fs::metadata(&args.file)
        .await
        .with_context(|| format!("failed to inspect artifact file {}", args.file.display()))?;
    if !meta.is_file() {
        bail!(
            "artifact path {} is not a regular file",
            args.file.display()
        );
    }
    if meta.len() == 0 {
        bail!("artifact file {} is empty", args.file.display());
    }
    let filename = match args.filename {
        Some(name) if !name.trim().is_empty() => name,
        Some(_) => bail!("--filename must not be empty"),
        None => args
            .file
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .context("could not determine filename from --file; pass --filename explicitly")?
            .to_owned(),
    };
    let content_type = args
        .content_type
        .unwrap_or_else(|| infer_content_type(&filename).to_owned());

    let file = tokio::fs::File::open(&args.file)
        .await
        .with_context(|| format!("failed to open artifact file {}", args.file.display()))?;
    let file_part = multipart::Part::stream_with_length(reqwest::Body::from(file), meta.len())
        .file_name(filename)
        .mime_str(&content_type)
        .with_context(|| format!("invalid MIME content type `{content_type}`"))?;

    let mut form = multipart::Form::new()
        .text("plate_id", args.plate.to_string())
        .text("use_ams", args.use_ams.to_string())
        .text("bed_leveling", args.bed_leveling.to_string())
        .text("auto_bed_leveling", args.auto_bed_leveling.to_string())
        .text("flow_cali", args.flow_cali.to_string())
        .text("auto_flow_cali", args.auto_flow_cali.to_string())
        .text("auto_offset_cali", args.auto_offset_cali.to_string())
        .text("timelapse", args.timelapse.to_string())
        .part("file", file_part);
    if let Some(value) = args.ams_mapping {
        form = form.text("ams_mapping", value);
    }
    if let Some(value) = args.ams_mapping2 {
        form = form.text("ams_mapping2", value);
    }
    if let Some(value) = args.ams_mapping_info {
        form = form.text("ams_mapping_info", value);
    }

    let url = format!(
        "{hub_url}/api/v1/tenants/{tenant_id}/printers/{}/jobs",
        args.printer_id
    );
    let response = http
        .post(&url)
        .bearer_auth(token.expose())
        .multipart(form)
        .send()
        .await
        .with_context(|| format!("failed to upload print job to {url}"))?;
    render_response(response, false).await
}

fn infer_content_type(filename: &str) -> &'static str {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".3mf") {
        "model/3mf"
    } else if lower.ends_with(".gcode") {
        "text/x-gcode"
    } else {
        "application/octet-stream"
    }
}

pub(super) async fn run_agents(
    http: &reqwest::Client,
    conn: &mut ResolvedConnection,
    cmd: AgentsCommand,
) -> Result<()> {
    let tenant_id = ensure_tenant_id(http, conn).await?;
    let token = require_token(conn)?;
    match cmd {
        AgentsCommand::List => {
            let url = format!("{}/api/v1/tenants/{tenant_id}/agents", conn.hub_url);
            send_simple(http, Method::GET, &url, token, None).await
        }
        AgentsCommand::RefreshPrinters { agent_id } => {
            validate_path_segment(&agent_id, "agent_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/agents/{agent_id}/refresh-printers",
                conn.hub_url
            );
            send_simple(http, Method::POST, &url, token, Some(b"{}".to_vec())).await
        }
        AgentsCommand::DiscoverPrinters {
            agent_id,
            timeout_seconds,
        } => {
            validate_path_segment(&agent_id, "agent_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/agents/{agent_id}/discover-printers",
                conn.hub_url
            );
            let body = serde_json::to_vec(&DiscoverBody { timeout_seconds })?;
            send_simple(http, Method::POST, &url, token, Some(body)).await
        }
        AgentsCommand::DiagnosePrinter { agent_id, serial } => {
            validate_path_segment(&agent_id, "agent_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/agents/{agent_id}/diagnose-printer",
                conn.hub_url
            );
            let body = serde_json::to_vec(&DiagnoseBody {
                serial_number: serial,
            })?;
            send_simple(http, Method::POST, &url, token, Some(body)).await
        }
    }
}

pub(super) async fn run_tokens(
    http: &reqwest::Client,
    conn: &mut ResolvedConnection,
    cmd: TokensCommand,
) -> Result<()> {
    let tenant_id = ensure_tenant_id(http, conn).await?;
    let token = require_token(conn)?;
    match cmd {
        TokensCommand::List => {
            let url = format!("{}/api/v1/tenants/{tenant_id}/tenant-tokens", conn.hub_url);
            send_simple(http, Method::GET, &url, token, None).await
        }
        TokensCommand::Create {
            name,
            scopes,
            expires_at,
        } => {
            let scopes = scopes
                .split(',')
                .map(str::trim)
                .filter(|scope| !scope.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            let url = format!("{}/api/v1/tenants/{tenant_id}/tenant-tokens", conn.hub_url);
            let body = serde_json::to_vec(&CreateTokenBody {
                name,
                scopes,
                expires_at,
            })?;
            send_simple(http, Method::POST, &url, token, Some(body)).await
        }
        TokensCommand::Rotate {
            token_id,
            expires_at,
        } => {
            validate_path_segment(&token_id, "token_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/tenant-tokens/{token_id}/rotate",
                conn.hub_url
            );
            let body = serde_json::to_vec(&RotateTokenBody { expires_at })?;
            send_simple(http, Method::POST, &url, token, Some(body)).await
        }
        TokensCommand::Revoke { token_id } => {
            validate_path_segment(&token_id, "token_id")?;
            let url = format!(
                "{}/api/v1/tenants/{tenant_id}/tenant-tokens/{token_id}",
                conn.hub_url
            );
            send_simple(http, Method::DELETE, &url, token, None).await
        }
    }
}
