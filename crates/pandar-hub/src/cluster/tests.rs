use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt;
use pandar_core::{AgentId, TenantId};
use serde::Deserialize;
use tokio::sync::Mutex;

use super::*;
use crate::db::DatabaseBackend;

#[derive(Debug, Default)]
struct RecordingNatsTransport {
    payloads: Mutex<Vec<Vec<u8>>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum OldShapeControlMessage {
    PrinterEvent {
        tenant_id: String,
        event: OldShapePrinterEvent,
    },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum OldShapePrinterEvent {
    #[serde(rename = "printer_snapshot")]
    PrinterSnapshot { printer: OldShapePrinter },
}

#[derive(Debug, Deserialize)]
struct OldShapePrinter {
    id: String,
    status: String,
}

#[async_trait]
impl NatsTransport for RecordingNatsTransport {
    async fn publish(&self, _subject: String, _payload: Vec<u8>) -> anyhow::Result<()> {
        Ok(())
    }

    async fn subscribe(&self, _subject: String) -> anyhow::Result<NatsPayloadStream> {
        let payloads = self
            .payloads
            .lock()
            .await
            .drain(..)
            .map(Ok)
            .collect::<Vec<_>>();
        Ok(Box::pin(futures_util::stream::iter(payloads)))
    }
}

#[test]
fn control_plane_config_rejects_sqlite_nats() {
    let err = ControlPlaneConfig::from_values(
        DatabaseBackend::Sqlite,
        Some("nats"),
        Some("nats://127.0.0.1:4222"),
        None,
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("SQLite deployments do not support"));
}

#[tokio::test]
async fn nats_control_plane_subscribe_reports_decode_errors_and_continues() {
    let transport = Arc::new(RecordingNatsTransport::default());
    let tenant_id = TenantId::new();
    let agent_id = AgentId::new();
    {
        let mut payloads = transport.payloads.lock().await;
        payloads.push(b"{".to_vec());
        payloads.push(
            serde_json::to_vec(&HubControlMessage::AgentWake {
                tenant_id: tenant_id.to_string(),
                agent_id: agent_id.to_string(),
            })
            .unwrap(),
        );
    }
    let control_plane = NatsControlPlane::new(transport, "pandar.test".to_string());

    let mut stream = control_plane.subscribe().await.unwrap();
    let err = stream.next().await.unwrap().unwrap_err();
    let message = stream.next().await.unwrap().unwrap();

    assert!(format!("{err:#}").contains("failed to decode control message"));
    assert!(matches!(
        message,
        HubControlMessage::AgentWake {
            tenant_id: decoded_tenant_id,
            agent_id: decoded_agent_id,
        } if decoded_tenant_id == tenant_id.to_string()
            && decoded_agent_id == agent_id.to_string()
    ));
}

#[test]
fn mixed_replica_control_plane_requires_compatibility_and_decodes_enrichment() {
    let tenant_id = TenantId::new();
    let mut current = serde_json::json!({
        "type": "printer_event",
        "tenant_id": tenant_id.to_string(),
        "event": {
            "type": "printer_snapshot",
            "printer": {
                "id": "printer-1",
                "tenant_id": tenant_id.to_string(),
                "agent_id": AgentId::new().to_string(),
                "serial_number": "SN-1",
                "printer_family": "bambu_lab",
                "name": "Printer",
                "model": null,
                "status": "RUNNING",
                "last_seen_at": "2026-07-10T00:00:00Z",
                "created_at": "2026-07-10T00:00:00Z",
                "nozzle_temperatures": [],
                "active_nozzle": null,
                "bed_temperature_celsius": null,
                "bed_target_temperature_celsius": null,
                "chamber_temperature_celsius": null,
                "chamber_light_on": null,
                "materials": null
            }
        }
    });
    assert!(serde_json::from_value::<HubControlMessage>(current.clone()).is_err());
    current["event"]["printer"]["compatibility"] =
        serde_json::to_value(pandar_core::compatibility::compatibility_for_model(None)).unwrap();
    let decoded_current: HubControlMessage = serde_json::from_value(current.clone()).unwrap();
    let HubControlMessage::PrinterEvent { event, .. } = decoded_current else {
        panic!("expected printer event")
    };
    let PrinterEvent::PrinterSnapshot { printer } = event else {
        panic!("expected printer snapshot")
    };
    assert_eq!(printer.state_revision, None);
    assert_eq!(printer.print, None);

    let printer = current["event"]["printer"].as_object_mut().unwrap();
    printer.insert("state_revision".to_owned(), serde_json::json!(12));
    printer.insert(
        "print".to_owned(),
        serde_json::json!({
            "task_generation": 4,
            "error_generation": 9,
            "job_state": 0,
            "gcode_state": "RUNNING",
            "task_id": null,
            "subtask_id": null,
            "progress_percent": 42,
            "remaining_time_minutes": 11,
            "current_layer": 2,
            "total_layers": 128,
            "gcode_file": null,
            "subtask_name": "Cube",
            "print_error": 83918929,
            "printer_job_id": "",
            "hms": [{"attr": 83887616, "code": 131184}]
        }),
    );
    let decoded_enriched: HubControlMessage = serde_json::from_value(current.clone()).unwrap();
    let HubControlMessage::PrinterEvent { event, .. } = decoded_enriched else {
        panic!("expected printer event")
    };
    let PrinterEvent::PrinterSnapshot { printer } = event else {
        panic!("expected printer snapshot")
    };
    assert_eq!(printer.state_revision, Some(12));
    assert_eq!(printer.print.as_ref().unwrap().job_state, Some(0));

    let old: OldShapeControlMessage = serde_json::from_value(current).unwrap();
    let OldShapeControlMessage::PrinterEvent {
        tenant_id: old_tenant,
        event,
    } = old;
    assert_eq!(old_tenant, tenant_id.to_string());
    let OldShapePrinterEvent::PrinterSnapshot { printer } = event;
    assert_eq!(printer.id, "printer-1");
    assert_eq!(printer.status, "RUNNING");
}
