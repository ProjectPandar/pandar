use std::time::Duration;

use anyhow::{Context, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tokio::{
    net::TcpStream,
    sync::mpsc,
    time::{Instant, interval_at, sleep, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};

use super::{
    MoonrakerClient, MoonrakerEndpoint,
    client::KlipperObjects,
    projection::{endpoint_snapshot, gcode_state, machine_snapshot, materials_json, print_report},
    status::{KlipperStatus, ObjectSelection},
};
use crate::{
    AgentConfig,
    commands::{printer_materials_snapshot_event, printer_snapshot_event},
    machine::{MaterialRefreshResult, mqtt::print_job_report_event},
};
use pandar_protocol::agent::v1::AgentEvent;

/// Klipper pushes temperature diffs several times a second; the Hub only needs
/// a periodic projection plus an immediate one on print-state transitions.
const EMIT_INTERVAL: Duration = Duration::from_secs(3);
/// Idle Klipper printers still stream temperature updates, so a silent socket
/// this long is treated as dead.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const RETRY_DELAY: Duration = Duration::from_secs(5);

const IDENTIFY_ID: u64 = 1;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Forwards Moonraker status for one printer until the session sender closes,
/// reconnecting after failures and reporting the printer offline meanwhile.
pub(super) async fn forward_reports_with_retry(
    config: AgentConfig,
    client: MoonrakerClient,
    endpoint: MoonrakerEndpoint,
    sender: mpsc::Sender<AgentEvent>,
) {
    let mut offline = false;
    loop {
        let result = forward_reports(&config, &client, &endpoint, &sender, &mut offline).await;
        if sender.is_closed() {
            return;
        }
        if let Err(error) = result {
            tracing::warn!(
                serial = %endpoint.serial,
                error = %redact(&format!("{error:#}"), &endpoint),
                "Moonraker report forwarding failed; retrying"
            );
        }
        if !offline {
            if sender
                .send(printer_snapshot_event(
                    &config,
                    endpoint_snapshot(&endpoint, Some("offline")),
                ))
                .await
                .is_err()
            {
                return;
            }
            offline = true;
        }
        tokio::select! {
            _ = sender.closed() => return,
            _ = sleep(RETRY_DELAY) => {}
        }
    }
}

pub(super) fn redact(message: &str, endpoint: &MoonrakerEndpoint) -> String {
    match endpoint.api_key.as_deref() {
        Some(key) if !key.is_empty() => message.replace(key, "[REDACTED_API_KEY]"),
        _ => message.to_owned(),
    }
}

async fn forward_reports(
    config: &AgentConfig,
    client: &MoonrakerClient,
    endpoint: &MoonrakerEndpoint,
    sender: &mpsc::Sender<AgentEvent>,
    offline: &mut bool,
) -> anyhow::Result<()> {
    let selection = ObjectSelection::from_available(
        &client.object_list().await.context("list Klipper objects")?,
    );
    let mut socket = connect(client).await?;
    send_request(
        &mut socket,
        IDENTIFY_ID,
        "server.connection.identify",
        IdentifyParams {
            client_name: "pandar-agent",
            version: &config.agent_version,
            client_type: "other",
            url: "https://github.com/ProjectPandar/pandar",
            api_key: client.api_key(),
        },
    )
    .await?;
    let mut next_id = IDENTIFY_ID + 1;
    let mut subscribe_id = subscribe(&mut socket, &selection, &mut next_id).await?;

    let mut status = KlipperStatus::default();
    let mut subscribed = false;
    let mut dirty = false;
    let mut last_state = None;
    let mut last_materials = None;
    let mut ticker = interval_at(Instant::now() + EMIT_INTERVAL, EMIT_INTERVAL);

    loop {
        tokio::select! {
            _ = sender.closed() => return Ok(()),
            _ = ticker.tick() => {
                if subscribed && dirty {
                    emit(config, endpoint, &selection, &status, sender, &mut last_materials).await?;
                    dirty = false;
                }
            }
            message = timeout(IDLE_TIMEOUT, socket.next()) => {
                let message = message
                    .context("Moonraker WebSocket went silent")?
                    .context("Moonraker WebSocket closed")?
                    .context("read Moonraker WebSocket message")?;
                let text = match message {
                    Message::Text(text) => text,
                    Message::Close(frame) => bail!("Moonraker closed the WebSocket: {frame:?}"),
                    _ => continue,
                };
                let message: RpcMessage = serde_json::from_str(&text)
                    .context("decode Moonraker JSON-RPC message")?;
                if let Some(error) = message.error {
                    bail!("Moonraker JSON-RPC request {:?} failed: {}", message.id, error.message);
                }
                match (message.id, message.method.as_deref()) {
                    (Some(id), _) if id == subscribe_id => {
                        let result: SubscribeResult = decode(message.result.as_deref(), "subscription result")?;
                        status.apply(result.status);
                        subscribed = true;
                        dirty = true;
                    }
                    (_, Some("notify_status_update")) => {
                        let (update, _eventtime): (KlipperObjects, f64) =
                            decode(message.params.as_deref(), "status update")?;
                        status.apply(update);
                        dirty = true;
                    }
                    // Klipper restarts drop the subscription; re-subscribe for a fresh status.
                    (_, Some("notify_klippy_ready")) => {
                        subscribe_id = subscribe(&mut socket, &selection, &mut next_id).await?;
                    }
                    (_, Some("notify_klippy_shutdown" | "notify_klippy_disconnected")) => {
                        status.apply(KlipperObjects::from([(
                            "webhooks".to_owned(),
                            serde_json::Map::from_iter([(
                                "state".to_owned(),
                                serde_json::Value::from("shutdown"),
                            )]),
                        )]));
                        dirty = true;
                    }
                    _ => continue,
                }
                if subscribed {
                    *offline = false;
                    let state = gcode_state(&status);
                    if last_state != Some(state) {
                        last_state = Some(state);
                        emit(config, endpoint, &selection, &status, sender, &mut last_materials).await?;
                        dirty = false;
                    }
                }
            }
        }
    }
}

async fn emit(
    config: &AgentConfig,
    endpoint: &MoonrakerEndpoint,
    selection: &ObjectSelection,
    status: &KlipperStatus,
    sender: &mpsc::Sender<AgentEvent>,
    last_materials: &mut Option<String>,
) -> anyhow::Result<()> {
    let observed_at = pandar_core::created_at_now();
    sender
        .send(printer_snapshot_event(
            config,
            machine_snapshot(endpoint, selection, status),
        ))
        .await
        .context("queue Moonraker printer snapshot")?;
    let materials_key = materials_json(selection, status, "");
    if materials_key.is_some() && materials_key != *last_materials {
        let printer_materials_json = materials_json(selection, status, &observed_at)
            .expect("materials projection is deterministic for the same status");
        sender
            .send(printer_materials_snapshot_event(
                config,
                MaterialRefreshResult {
                    serial: endpoint.serial.clone(),
                    printer_id: None,
                    printer_materials_json,
                },
            ))
            .await
            .context("queue Moonraker materials snapshot")?;
        *last_materials = materials_key;
    }
    sender
        .send(print_job_report_event(
            config,
            print_report(endpoint, status, observed_at, String::new()),
        ))
        .await
        .context("queue Moonraker print report")
}

async fn connect(client: &MoonrakerClient) -> anyhow::Result<Socket> {
    let mut request = client
        .websocket_url()
        .into_client_request()
        .context("build Moonraker WebSocket request")?;
    if let Some(key) = client.api_key() {
        request.headers_mut().insert(
            "X-Api-Key",
            HeaderValue::from_str(key).context("encode Moonraker API key header")?,
        );
    }
    let (socket, _) = connect_async(request)
        .await
        .context("connect Moonraker WebSocket")?;
    Ok(socket)
}

async fn subscribe(
    socket: &mut Socket,
    selection: &ObjectSelection,
    next_id: &mut u64,
) -> anyhow::Result<u64> {
    let id = *next_id;
    *next_id += 1;
    send_request(
        socket,
        id,
        "printer.objects.subscribe",
        SubscribeParams {
            objects: selection
                .subscribed
                .iter()
                .map(|name| (name.as_str(), None))
                .collect(),
        },
    )
    .await?;
    Ok(id)
}

async fn send_request<P: Serialize>(
    socket: &mut Socket,
    id: u64,
    method: &str,
    params: P,
) -> anyhow::Result<()> {
    let request = serde_json::to_string(&RpcRequest {
        jsonrpc: "2.0",
        method,
        params,
        id,
    })
    .expect("Moonraker JSON-RPC request is serializable");
    socket
        .send(Message::text(request))
        .await
        .with_context(|| format!("send Moonraker {method} request"))
}

fn decode<T: for<'de> Deserialize<'de>>(raw: Option<&RawValue>, what: &str) -> anyhow::Result<T> {
    let raw = raw.with_context(|| format!("Moonraker {what} is missing"))?;
    serde_json::from_str(raw.get()).with_context(|| format!("decode Moonraker {what}"))
}

#[derive(Serialize)]
struct RpcRequest<'a, P> {
    jsonrpc: &'static str,
    method: &'a str,
    params: P,
    id: u64,
}

#[derive(Serialize)]
struct IdentifyParams<'a> {
    client_name: &'static str,
    version: &'a str,
    #[serde(rename = "type")]
    client_type: &'static str,
    url: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<&'a str>,
}

#[derive(Serialize)]
struct SubscribeParams<'a> {
    objects: std::collections::BTreeMap<&'a str, Option<()>>,
}

#[derive(Deserialize)]
struct RpcMessage {
    id: Option<u64>,
    method: Option<String>,
    params: Option<Box<RawValue>>,
    result: Option<Box<RawValue>>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    message: String,
}

#[derive(Deserialize)]
struct SubscribeResult {
    status: KlipperObjects,
}
