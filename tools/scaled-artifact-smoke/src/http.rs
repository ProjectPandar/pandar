use std::net::SocketAddr;

use anyhow::{Context, ensure};
use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use futures_util::StreamExt;
use http_body_util::BodyExt;
use pandar_agent::{
    AgentConfig,
    commands::{ArtifactReader, HubArtifactReader},
};
use pandar_hub::{AppState, router};
use serde::Deserialize;
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};
use tower::ServiceExt;

use crate::fixture::{ARTIFACT_BYTES, SmokeFixture};

#[derive(Deserialize)]
struct PrinterEventTicket {
    ticket: String,
}

#[derive(Deserialize)]
struct PrinterEventEnvelope {
    r#type: String,
}

pub async fn create_print_through_multipart_route(
    state: &AppState,
    fixture: &SmokeFixture,
    printer_id: &str,
) -> anyhow::Result<()> {
    let (status, body) = create_print(state, fixture, printer_id).await?;
    ensure!(
        status == StatusCode::CREATED,
        "multipart print route returned {status}: {}",
        String::from_utf8_lossy(&body)
    );
    Ok(())
}

pub async fn create_print(
    state: &AppState,
    fixture: &SmokeFixture,
    printer_id: &str,
) -> anyhow::Result<(StatusCode, Vec<u8>)> {
    let body = multipart_print_body(printer_id, "scaled-smoke.3mf");
    let request = Request::builder()
        .method("POST")
        .uri("/api/v1/plugin/prints")
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.plugin_token),
        )
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={}", body.boundary),
        )
        .body(Body::from(body.body))
        .context("build multipart print request")?;
    let response = router(state.clone())
        .oneshot(request)
        .await
        .context("send multipart print request")?;
    let status = response.status();
    let body = response.into_body().collect().await?.to_bytes();
    Ok((status, body.to_vec()))
}

pub async fn create_ws_ticket(
    state: &AppState,
    tenant_id: pandar_core::TenantId,
    bearer: &str,
) -> anyhow::Result<String> {
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/v1/tenants/{tenant_id}/printer-events/tickets"
        ))
        .header(header::AUTHORIZATION, format!("Bearer {bearer}"))
        .body(Body::empty())
        .context("build printer event ticket request")?;
    let response = router(state.clone())
        .oneshot(request)
        .await
        .context("send printer event ticket request")?;
    let status = response.status();
    let body = response.into_body().collect().await?.to_bytes();
    ensure!(
        status == StatusCode::OK,
        "printer event ticket route returned {status}: {}",
        String::from_utf8_lossy(&body)
    );
    let ticket: PrinterEventTicket =
        serde_json::from_slice(&body).context("decode printer event ticket body")?;
    Ok(ticket.ticket)
}

pub async fn download_artifact_route(
    state: &AppState,
    fixture: &SmokeFixture,
    path: &str,
) -> anyhow::Result<(StatusCode, Vec<u8>)> {
    let request = Request::builder()
        .method("GET")
        .uri(path)
        .header(
            header::AUTHORIZATION,
            format!("Bearer {}", fixture.agent_credential),
        )
        .body(Body::empty())
        .context("build artifact download request")?;
    let response = router(state.clone())
        .oneshot(request)
        .await
        .context("send artifact download request")?;
    let status = response.status();
    let body = response.into_body().collect().await?.to_bytes().to_vec();
    Ok((status, body))
}

pub async fn download_artifact(
    state: &AppState,
    fixture: &SmokeFixture,
    path: &str,
) -> anyhow::Result<()> {
    let base_url = serve_hub(state.clone()).await?;
    let agent_config = AgentConfig {
        hub_grpc_url: "grpc://unused-in-smoke".to_owned(),
        hub_api_url: Some(base_url),
        agent_name: "scaled-smoke-agent".to_owned(),
        agent_id: fixture.agent_id.to_string(),
        tenant_id: fixture.tenant_id.to_string(),
        agent_credential: fixture.agent_credential.clone(),
        agent_version: "scaled-smoke".to_owned(),
        printers: "[]".to_owned(),
    };
    let downloaded = HubArtifactReader::new(&agent_config)
        .read_artifact(path)
        .await?;
    ensure!(
        downloaded == ARTIFACT_BYTES,
        "downloaded artifact bytes differ"
    );
    Ok(())
}

pub async fn connect_ws_with_ticket(
    base_url: &str,
    tenant_id: pandar_core::TenantId,
    ticket: &str,
) -> anyhow::Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let ws_url = format!(
        "{}/api/v1/tenants/{tenant_id}/printer-events?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    );
    let request = ws_url.into_client_request()?;
    let (ws, _) = connect_async(request).await.context("connect websocket")?;
    Ok(ws)
}

pub async fn next_ws_event_type(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
) -> anyhow::Result<String> {
    let message = tokio::time::timeout(std::time::Duration::from_secs(10), ws.next())
        .await
        .context("websocket event timed out")?
        .context("websocket closed before event")?
        .context("websocket read failed")?;
    let text = message.into_text().context("expected websocket text")?;
    let event: PrinterEventEnvelope =
        serde_json::from_str(&text).context("decode websocket event")?;
    Ok(event.r#type)
}

pub async fn serve_hub(state: AppState) -> anyhow::Result<String> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, router(state)).await {
            eprintln!("scaled smoke hub server failed: {err:#}");
        }
    });
    Ok(format!("http://{addr}"))
}

struct MultipartBody {
    boundary: String,
    body: Vec<u8>,
}

fn multipart_print_body(printer_id: &str, filename: &str) -> MultipartBody {
    let boundary = "pandar-scaled-smoke-boundary";
    let mut body = Vec::new();
    // The complete field set Bambu Studio submits for a cloud print.
    for (name, value) in [
        ("printer_id", printer_id),
        ("filename", filename),
        ("content_type", "model/3mf"),
        ("plate_id", "1"),
        ("use_ams", "true"),
        ("bed_leveling", "false"),
        ("auto_bed_leveling", "0"),
        ("flow_cali", "false"),
        ("auto_flow_cali", "0"),
        ("auto_offset_cali", "0"),
        ("timelapse", "true"),
        ("task_name", filename),
        ("project_name", "Scaled smoke project"),
        ("preset_name", "Scaled smoke preset"),
        ("config_plate_index", "1"),
        ("nozzle_mapping", "[]"),
        ("ams_mapping", "[]"),
        ("ams_mapping2", "[]"),
        ("ams_mapping_info", "[]"),
        ("nozzles_info", "[]"),
        ("connection_type", "cloud"),
        ("comments", ""),
        ("origin_profile_id", "0"),
        ("stl_design_id", "0"),
        ("origin_model_id", ""),
        ("print_type", "from_normal"),
        ("dev_name", "Scaled Smoke Printer"),
        ("vibration_cali", "false"),
        ("layer_inspect", "false"),
        ("timelapse_use_internal", "false"),
        ("bed_type", "auto"),
        ("extruder_cali_manual_mode", "-1"),
        ("try_emmc_print", "false"),
        ("svc_context", ""),
        ("slicer_uid", "scaled-smoke"),
    ] {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(b"Content-Type: model/3mf\r\n\r\n");
    body.extend_from_slice(ARTIFACT_BYTES);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

    MultipartBody {
        boundary: boundary.to_owned(),
        body,
    }
}
