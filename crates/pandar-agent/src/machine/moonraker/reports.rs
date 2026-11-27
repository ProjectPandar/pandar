use std::time::Duration;

use anyhow::{Context, bail};
use tokio::{
    sync::mpsc,
    time::{Instant, interval_at, sleep, timeout},
};

use super::{
    MoonrakerClient, MoonrakerEndpoint,
    client::KlipperObjects,
    projection::{endpoint_snapshot, gcode_state, machine_snapshot, materials_json, print_report},
    status::{KlipperStatus, ObjectSelection, PrintStats},
    websocket::{IDENTIFY_ID, SubscribeResult, connect, decode, identify, read_rpc, subscribe},
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
    identify(&mut socket, client, &config.agent_version).await?;
    let mut next_id = IDENTIFY_ID + 1;
    let mut subscribe_id = subscribe(&mut socket, &selection, &mut next_id).await?;

    let mut status = KlipperStatus::default();
    let mut subscribed = false;
    let mut dirty = false;
    let mut last_state = None;
    let mut last_materials = None;
    let mut estimate = PrintEstimate::default();
    let mut ticker = interval_at(Instant::now() + EMIT_INTERVAL, EMIT_INTERVAL);

    loop {
        tokio::select! {
            _ = sender.closed() => return Ok(()),
            _ = ticker.tick() => {
                if subscribed && dirty {
                    estimate.refresh(client, endpoint, &status).await;
                    emit(config, endpoint, &selection, &status, estimate.estimated_time_seconds, sender, &mut last_materials).await?;
                    dirty = false;
                }
            }
            message = timeout(IDLE_TIMEOUT, read_rpc(&mut socket)) => {
                let message = message
                    .context("Moonraker WebSocket went silent")??;
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
                    // Scripts dispatched in the background report through these responses.
                    (_, Some("notify_gcode_response")) => {
                        if let Ok(([response],)) =
                            decode::<([String; 1],)>(message.params.as_deref(), "G-code response")
                        {
                            tracing::info!(serial = %endpoint.serial, response, "Moonraker G-code response");
                        }
                    }
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
                        estimate.refresh(client, endpoint, &status).await;
                        emit(config, endpoint, &selection, &status, estimate.estimated_time_seconds, sender, &mut last_materials).await?;
                        dirty = false;
                    }
                }
            }
        }
    }
}

/// The slicer's `estimated_time` for the file being printed, fetched once per
/// print so the remaining time does not rely on progress extrapolation alone.
#[derive(Default)]
struct PrintEstimate {
    filename: String,
    estimated_time_seconds: Option<f64>,
}

impl PrintEstimate {
    async fn refresh(
        &mut self,
        client: &MoonrakerClient,
        endpoint: &MoonrakerEndpoint,
        status: &KlipperStatus,
    ) {
        let print_stats: PrintStats = status.object("print_stats");
        let filename = print_stats.filename.trim().to_owned();
        if filename == self.filename {
            return;
        }
        self.filename = filename.clone();
        self.estimated_time_seconds = if filename.is_empty() {
            None
        } else {
            match client.file_metadata(&filename).await {
                Ok(metadata) => metadata.estimated_time,
                Err(error) => {
                    tracing::warn!(
                        serial = %endpoint.serial,
                        error = %redact(&format!("{error:#}"), endpoint),
                        "Moonraker file metadata unavailable; remaining time falls back to progress"
                    );
                    None
                }
            }
        };
    }
}

async fn emit(
    config: &AgentConfig,
    endpoint: &MoonrakerEndpoint,
    selection: &ObjectSelection,
    status: &KlipperStatus,
    estimated_time_seconds: Option<f64>,
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
            print_report(
                endpoint,
                status,
                observed_at,
                String::new(),
                estimated_time_seconds,
            ),
        ))
        .await
        .context("queue Moonraker print report")
}
