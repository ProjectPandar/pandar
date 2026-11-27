//! Opt-in run against a real Moonraker printer (a Snapmaker U1 by default):
//!
//! ```sh
//! PANDAR_MOONRAKER_HOST=10.1.61.14:7125 [PANDAR_MOONRAKER_API_KEY=...] \
//!   [PANDAR_MOONRAKER_ACTUATE=fans,light,heat,tool,dwell] \
//!   cargo nextest run -p pandar-agent --run-ignored only -E test(hardware)
//! ```
//!
//! Without the host the test returns immediately. It links the printer through
//! the real runtime, reads telemetry for a few seconds, runs the diagnostic,
//! streams one camera frame and, per `PANDAR_MOONRAKER_ACTUATE`, exercises
//! controls — restoring every value it changed. Every step is recorded before
//! anything is asserted, so a failed run still leaves its transcript in
//! `target/e2e-artifacts/moonraker-hardware.json`.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::time::timeout;

use super::support::{Harness, fan_kind, link, snapshot_summary, write_artifact};
use crate::machine::{BambuMachineGateway, CameraSource};
use pandar_protocol::agent::v1::{
    DiagnosePrinter, GcodeLineOperation, PrinterSnapshot, SelectExtruderOperation,
    SetChamberLightOperation, SetFanSpeedOperation, SetHotendTemperatureOperation, agent_event,
    hub_command, printer_operation,
};

const HOST_VAR: &str = "PANDAR_MOONRAKER_HOST";
const API_KEY_VAR: &str = "PANDAR_MOONRAKER_API_KEY";
const ACTUATE_VAR: &str = "PANDAR_MOONRAKER_ACTUATE";
const TELEMETRY_WAIT: Duration = Duration::from_secs(6);
const HEAT_WAIT: Duration = Duration::from_secs(30);
const TOOL_CHANGE_WAIT: Duration = Duration::from_secs(120);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "needs a real Moonraker printer; set PANDAR_MOONRAKER_HOST"]
async fn moonraker_hardware_transcript() {
    let Ok(host) = std::env::var(HOST_VAR) else {
        return;
    };
    let api_key = std::env::var(API_KEY_VAR).unwrap_or_default();
    let tiers = std::env::var(ACTUATE_VAR).unwrap_or_default();
    let actuate = tiers
        .split(',')
        .map(str::trim)
        .filter(|tier| !tier.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_test_writer()
        .try_init();

    let mut harness = Harness::start().await;
    let link_result = harness.dispatch("link", link(&host, &api_key)).await;
    assert!(link_result.success, "{link_result:?}");
    let serial = serde_json::from_str::<Value>(&link_result.result_json).unwrap()["serial_number"]
        .as_str()
        .unwrap()
        .to_owned();

    let first = wait_for_snapshot(&mut harness, |snapshot| snapshot.cooling_system.is_some()).await;
    let materials = harness
        .find(|event| match event {
            agent_event::Event::PrinterMaterialsSnapshot(materials) => {
                Some(serde_json::from_str::<Value>(&materials.printer_materials_json).unwrap())
            }
            _ => None,
        })
        .await;
    let report = harness
        .report(|report| !report.gcode_state.is_empty())
        .await;

    let diagnosed = harness
        .dispatch(
            "diagnose",
            hub_command::Command::DiagnosePrinter(DiagnosePrinter {
                serial_number: serial.clone(),
            }),
        )
        .await;
    let diagnostic = serde_json::from_str::<Value>(&diagnosed.result_json).unwrap();

    let camera = match harness.gateway.camera_source(&serial).await.unwrap() {
        CameraSource::Moonraker(source) => source,
        CameraSource::Bambu(_) => panic!("camera must resolve through Moonraker"),
    };
    let camera_summary = json!({
        "served_from": camera.client.base_url(),
        "snapshot_url": camera.snapshot_url,
        "stream_url": camera.stream_url,
    });
    let (frames, mut frame_receiver) = tokio::sync::mpsc::channel(1);
    let stream = tokio::spawn(camera.stream_mjpeg(frames));
    let frame = timeout(Duration::from_secs(10), frame_receiver.recv()).await;
    stream.abort();

    let mut controls = Vec::new();
    for tier in &actuate {
        controls.push(match tier.as_str() {
            "light" => exercise_light(&mut harness, &serial, &first).await,
            "fans" => exercise_fans(&mut harness, &serial, &first).await,
            "heat" => exercise_heat(&mut harness, &serial).await,
            "tool" => exercise_tool(&mut harness, &serial, &first).await,
            "dwell" => exercise_dwell(&mut harness, &serial).await,
            other => panic!("unknown actuation tier {other}"),
        });
    }

    let transcript = json!({
        "host": host,
        "link": serde_json::from_str::<Value>(&link_result.result_json).unwrap(),
        "snapshot": snapshot_summary(&first),
        "materials": materials,
        "report": {
            "state": report.gcode_state,
            "file": report.gcode_file,
            "layers": [report.current_layer, report.total_layers],
        },
        "diagnose": {
            "overall": diagnostic["overall"],
            "checks": diagnostic["checks"].as_array().unwrap().iter()
                .map(|check| json!([check["id"], check["status"], check["message"]]))
                .collect::<Vec<_>>(),
        },
        "camera": {
            "resolved": camera_summary,
            "frame_bytes": frame.ok().flatten().map(|frame| frame.len()),
        },
        "controls": controls,
    });
    write_artifact("moonraker-hardware.json", &transcript);

    let failures = failures(&transcript);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every `"command"` entry in the transcript that the Agent did not report as
/// successful, or whose expected telemetry never showed up (`"observed": null`).
fn failures(value: &Value) -> Vec<String> {
    match value {
        Value::Object(map) => {
            let mut found = Vec::new();
            if let Some(command) = map.get("command")
                && command["success"] != Value::Bool(true)
            {
                found.push(format!("{command}"));
            }
            if map.get("observed") == Some(&Value::Null) {
                found.push(format!("telemetry never showed {map:?}"));
            }
            for (_, child) in map {
                found.extend(failures(child));
            }
            found
        }
        Value::Array(items) => items.iter().flat_map(failures).collect(),
        _ => Vec::new(),
    }
}

async fn exercise_light(harness: &mut Harness, serial: &str, current: &PrinterSnapshot) -> Value {
    let original = current.chamber_light_on.unwrap_or(false);
    let mut steps = Vec::new();
    for on in [!original, original] {
        let command = run(
            harness,
            serial,
            printer_operation::Operation::SetChamberLight(SetChamberLightOperation { on }),
        )
        .await;
        let observed = harness
            .find_within(TELEMETRY_WAIT, |event| match event {
                agent_event::Event::PrinterSnapshot(snapshot)
                    if snapshot.chamber_light_on == Some(on) =>
                {
                    Some(snapshot.chamber_light_on)
                }
                _ => None,
            })
            .await;
        steps.push(json!({"set": on, "command": command, "observed": observed}));
    }
    json!({"tier": "light", "original": original, "steps": steps})
}

async fn exercise_fans(harness: &mut Harness, serial: &str, current: &PrinterSnapshot) -> Value {
    let originals = current.cooling_system.as_ref().unwrap().fans.clone();
    let mut steps = Vec::new();
    for fan in originals
        .iter()
        .filter(|fan| control_index(fan.kind).is_some())
    {
        let index = control_index(fan.kind).unwrap();
        for percent in [50_u8, fan.speed_percent as u8] {
            let command = run(
                harness,
                serial,
                printer_operation::Operation::SetFanSpeed(SetFanSpeedOperation {
                    fan_index: u32::from(index),
                    speed_percent: u32::from(percent),
                    airduct: false,
                }),
            )
            .await;
            // Some fans are owned by firmware logic (the U1's purifier drives
            // `exhaust_fan`), so a command may be accepted without the speed
            // following; record whether telemetry confirmed it.
            let followed = harness
                .find_within(TELEMETRY_WAIT, |event| match event {
                    agent_event::Event::PrinterSnapshot(snapshot) => snapshot
                        .cooling_system
                        .as_ref()
                        .is_some_and(|cooling| {
                            cooling.fans.iter().any(|candidate| {
                                candidate.kind == fan.kind
                                    && candidate.speed_percent == u32::from(percent)
                            })
                        })
                        .then(|| snapshot.clone()),
                    _ => None,
                })
                .await;
            steps.push(json!({
                "fan": fan_kind(fan.kind),
                "set": percent,
                "command": command,
                "fans": fans_summary(&followed.unwrap_or_else(|| current.clone())),
            }));
        }
    }
    assert!(
        steps
            .iter()
            .any(|step| step["fans"] != fans_summary(current)),
        "no fan followed its commanded speed: {steps:?}"
    );
    json!({"tier": "fans", "steps": steps})
}

async fn exercise_heat(harness: &mut Harness, serial: &str) -> Value {
    let mut steps = Vec::new();
    for target in [60_u32, 0] {
        let command = run(
            harness,
            serial,
            printer_operation::Operation::SetHotendTemperature(SetHotendTemperatureOperation {
                temperature_celsius: target,
                wait: false,
                extruder_id: Some(1),
            }),
        )
        .await;
        let observed = harness
            .find_within(HEAT_WAIT, |event| match event {
                agent_event::Event::PrinterSnapshot(snapshot) => snapshot
                    .nozzle_temperatures
                    .iter()
                    .any(|nozzle| {
                        nozzle.label == "T1"
                            && nozzle.target_celsius.trim().parse::<f64>() == Ok(f64::from(target))
                    })
                    .then(|| nozzle_targets(snapshot)),
                _ => None,
            })
            .await;
        steps.push(json!({"target": target, "command": command, "observed": observed}));
    }
    json!({"tier": "heat", "steps": steps})
}

async fn exercise_tool(harness: &mut Harness, serial: &str, current: &PrinterSnapshot) -> Value {
    let original = current
        .active_nozzle
        .trim_start_matches('T')
        .parse::<u32>()
        .unwrap_or(0);
    let mut steps = Vec::new();
    // The original tool is always restored, even when the first switch is not
    // confirmed within the window.
    for tool in [u32::from(original == 0), original] {
        let command = run(
            harness,
            serial,
            printer_operation::Operation::SelectExtruder(SelectExtruderOperation {
                extruder_id: tool,
            }),
        )
        .await;
        let observed = harness
            .find_within(TOOL_CHANGE_WAIT, |event| match event {
                agent_event::Event::PrinterSnapshot(snapshot)
                    if snapshot.active_nozzle == format!("T{tool}") =>
                {
                    Some(snapshot.active_nozzle.clone())
                }
                _ => None,
            })
            .await;
        steps.push(json!({"select": tool, "command": command, "observed": observed}));
    }
    json!({"tier": "tool", "original": format!("T{original}"), "steps": steps})
}

/// A dwell longer than the dispatch grace: Klipper answers after 5 s, so the
/// Agent must report the script dispatched within the grace and leave the
/// request running.
async fn exercise_dwell(harness: &mut Harness, serial: &str) -> Value {
    let started = std::time::Instant::now();
    let command = run(
        harness,
        serial,
        printer_operation::Operation::GcodeLine(GcodeLineOperation {
            param: "G4 P5000".to_owned(),
        }),
    )
    .await;
    let elapsed = started.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "the dwell must be dispatched before it finishes: {elapsed:?}"
    );
    tokio::time::sleep(Duration::from_secs(5) - elapsed).await;
    json!({"tier": "dwell", "command": command, "elapsed_seconds": elapsed.as_secs()})
}

/// Sends a control and returns the Agent's command result for the transcript.
async fn run(
    harness: &mut Harness,
    serial: &str,
    operation: printer_operation::Operation,
) -> Value {
    let result = harness
        .dispatch("control", operate(serial, operation))
        .await;
    json!({"success": result.success, "error": result.error})
}

async fn wait_for_snapshot(
    harness: &mut Harness,
    predicate: impl Fn(&PrinterSnapshot) -> bool,
) -> PrinterSnapshot {
    harness
        .find(|event| match event {
            agent_event::Event::PrinterSnapshot(snapshot) if predicate(snapshot) => {
                Some(snapshot.clone())
            }
            _ => None,
        })
        .await
}

fn operate(serial: &str, operation: printer_operation::Operation) -> hub_command::Command {
    hub_command::Command::PrinterOperation(pandar_protocol::agent::v1::PrinterOperation {
        serial_number: serial.to_owned(),
        required_device_features: Vec::new(),
        operation: Some(operation),
    })
}

/// Pandar's controllable fan index for a proto fan kind, when it has one.
fn control_index(kind: i32) -> Option<u8> {
    match fan_kind(kind).as_str() {
        "part_cooling" => Some(1),
        "auxiliary" => Some(2),
        "chamber" => Some(3),
        _ => None,
    }
}

fn fans_summary(snapshot: &PrinterSnapshot) -> Value {
    json!(
        snapshot
            .cooling_system
            .as_ref()
            .unwrap()
            .fans
            .iter()
            .map(|fan| json!([fan_kind(fan.kind), fan.speed_percent]))
            .collect::<Vec<_>>()
    )
}

fn nozzle_targets(snapshot: &PrinterSnapshot) -> Value {
    json!(
        snapshot
            .nozzle_temperatures
            .iter()
            .map(|nozzle| json!([nozzle.label, nozzle.current_celsius, nozzle.target_celsius]))
            .collect::<Vec<_>>()
    )
}
