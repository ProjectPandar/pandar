//! End-to-end: Hub commands drive the real Agent command handlers and runtime
//! gateway against an in-process Moonraker modelled on a Snapmaker U1 target.
//! The run writes `target/e2e-artifacts/moonraker-agent.json`, a transcript
//! of everything the printer received and the Agent reported.

mod camera_fallback;
mod controls;
mod fake_moonraker;
mod filament_commands;
mod hardware;
mod support;

use std::time::Duration;

use serde_json::{Value, json};
use tokio::{sync::mpsc, time::timeout};

use crate::machine::{BambuMachineGateway, CameraSource};
use fake_moonraker::{API_KEY, CPU_SERIAL, FakeMoonraker, JPEG_FRAME};
use pandar_protocol::agent::v1::{
    DiagnosePrinter, PrinterFamily, StopOperation, agent_event, hub_command, printer_operation,
};
use support::{
    GCODE, Harness, JOB_ID, SERIAL, link, operate, print_command, report_summary, snapshot_summary,
    write_artifact,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moonraker_link_report_print_control_diagnose_and_camera() {
    assert_eq!(CPU_SERIAL.to_ascii_uppercase(), SERIAL);
    let printer = FakeMoonraker::start().await;
    let mut harness = Harness::start().await;

    harness
        .run("link-wrong-key", link(&printer.host, "wrong-key"))
        .await;
    let rejected = harness.result("link-wrong-key").await;
    assert!(!rejected.error.contains("wrong-key"), "{}", rejected.error);

    harness.run("link", link(&printer.host, API_KEY)).await;
    let linked = harness.result("link").await;
    let linked_snapshot = harness
        .find(|event| match event {
            agent_event::Event::PrinterSnapshot(snapshot) if snapshot.connection_authoritative => {
                Some(snapshot.clone())
            }
            _ => None,
        })
        .await;
    assert_eq!(
        linked_snapshot.printer_family,
        PrinterFamily::Moonraker as i32
    );
    assert_eq!(linked_snapshot.access_code, API_KEY);
    let materials = harness
        .find(|event| match event {
            agent_event::Event::PrinterMaterialsSnapshot(materials) => {
                Some(serde_json::from_str::<Value>(&materials.printer_materials_json).unwrap())
            }
            _ => None,
        })
        .await;
    let idle = harness.report(|report| report.gcode_state == "IDLE").await;

    harness.run("print", print_command()).await;
    let printed = harness.result("print").await;
    let running = harness
        .report(|report| report.gcode_state == "RUNNING" && report.job_id == JOB_ID)
        .await;

    let operations = controls::run_control_operations(&mut harness, &printer).await;
    let paused = harness
        .report(|report| report.gcode_state == "PAUSE" && report.job_id == JOB_ID)
        .await;
    let controlled = harness
        .find(|event| match event {
            agent_event::Event::PrinterSnapshot(snapshot)
                if snapshot.active_nozzle == "T3"
                    && snapshot.chamber_light_on == Some(true)
                    && snapshot
                        .nozzle_temperatures
                        .iter()
                        .any(|nozzle| nozzle.label == "T2" && nozzle.target_celsius == "220.0")
                    && snapshot.cooling_system.is_some() =>
            {
                Some(snapshot.clone())
            }
            _ => None,
        })
        .await;

    harness
        .run(
            "diagnose",
            hub_command::Command::DiagnosePrinter(DiagnosePrinter {
                serial_number: SERIAL.to_owned(),
            }),
        )
        .await;
    let diagnosed = harness.result("diagnose").await;
    let diagnostic = serde_json::from_str::<Value>(&diagnosed.result_json).unwrap();

    let camera = match harness.gateway.camera_source(SERIAL).await.unwrap() {
        CameraSource::Moonraker(source) => source,
        CameraSource::Bambu(_) => panic!("U1 camera must resolve through Moonraker"),
    };
    let (frames, mut frame_receiver) = mpsc::channel(1);
    let stream = tokio::spawn(camera.stream_mjpeg(frames));
    let frame = timeout(Duration::from_secs(5), frame_receiver.recv())
        .await
        .unwrap()
        .unwrap();
    stream.abort();
    assert!(
        frame
            .windows(JPEG_FRAME.len())
            .any(|window| window == JPEG_FRAME)
    );

    harness
        .run(
            "stop",
            operate(printer_operation::Operation::Stop(StopOperation {})),
        )
        .await;
    let stopped = harness.result("stop").await;
    let cancelled = harness
        .report(|report| report.gcode_state == "IDLE" && report.job_id == JOB_ID)
        .await;

    let transcript = json!({
        "link_rejected": {
            "success": rejected.success,
            "result": serde_json::from_str::<Value>(&rejected.result_json).unwrap(),
        },
        "link": serde_json::from_str::<Value>(&linked.result_json).unwrap(),
        "linked_snapshot": snapshot_summary(&linked_snapshot),
        "websocket_identified_with_api_key": printer.identified_with_api_key().await,
        "materials": {
            "units": materials["ams_units"].as_array().unwrap().iter().map(|unit| json!({
                "unit_id": unit["unit_id"],
                "trays": unit["trays"].as_array().unwrap().iter().map(|tray| json!([
                    tray["tray_id"], tray["exists"], tray["type"], tray["color"], tray["name"],
                ])).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "active_tray": materials["active_tray"],
        },
        "idle_report": report_summary(&idle),
        "print": serde_json::from_str::<Value>(&printed.result_json).unwrap()
            .get("uploaded_path").cloned(),
        "running_report": report_summary(&running),
        "operations": operations,
        "paused_report": report_summary(&paused),
        "control_snapshot": snapshot_summary(&controlled),
        "diagnose": {
            "overall": diagnostic["overall"],
            "checks": diagnostic["checks"].as_array().unwrap().iter()
                .map(|check| json!([check["id"], check["status"]]))
                .collect::<Vec<_>>(),
        },
        "camera": {
            "frame_bytes": frame.len(),
            "webcam_requests": printer.webcam_requests().await,
        },
        "stop": stopped.success,
        "cancelled_report": report_summary(&cancelled),
        "printer_requests": printer.requests().await,
    });
    write_artifact("moonraker-agent.json", &transcript);

    assert_eq!(
        transcript,
        json!({
            "link_rejected": {
                "success": false,
                "result": {"type": "printer_link_error", "error_code": "invalid_access_code"},
            },
            "link": {
                "type": "printer_link",
                "serial_number": SERIAL,
                "host": printer.host,
                "name": "Workshop U1",
                "model": "Snapmaker U1",
                "status": "IDLE",
            },
            "linked_snapshot": {
                "serial": SERIAL,
                "model": "Snapmaker U1",
                "state": "IDLE",
                "active_nozzle": "T0",
                "nozzles": [
                    ["T0", "25.0", "0.0"], ["T1", "25.5", "0.0"],
                    ["T2", "26.0", "0.0"], ["T3", "26.5", "0.0"],
                ],
                "diameters": ["0.4", "0.6", "0.4", "0.8"],
                "bed_celsius": "24.5",
                "chamber_light_on": false,
                "fans": [
                    ["hotend", 100], ["part_cooling", 100], ["chamber", 30],
                    ["hotend_second", 80], ["controller", 100],
                ],
            },
            "websocket_identified_with_api_key": true,
            "materials": {
                "units": [{
                    "unit_id": "0",
                    "trays": [
                        ["0", true, "PLA", "FF0000FF", "Snapmaker PLA Basic"],
                        ["1", true, "PETG", "00FF00FF", "Generic PETG"],
                        ["2", false, null, null, null],
                        ["3", false, null, null, null],
                    ],
                }],
                "active_tray": {"kind": "ams", "global_tray_id": 0, "ams_id": "0", "tray_id": "0"},
            },
            "idle_report": {
                "state": "IDLE", "job_id": "", "percent": 0, "file": "", "layers": [0, 0],
                "remaining_minutes": 0,
            },
            "print": format!("pandar/{JOB_ID}/benchy.gcode"),
            "running_report": {
                "state": "RUNNING", "job_id": JOB_ID, "percent": 25,
                "file": format!("pandar/{JOB_ID}/benchy.gcode"), "layers": [3, 120],
                "remaining_minutes": 60,
            },
            "operations": [
                {"command": "pause", "success": true, "elapsed_seconds": null},
                {"command": "bed", "success": true, "elapsed_seconds": null},
                {"command": "light", "success": true, "elapsed_seconds": null},
                {"command": "home", "success": true, "elapsed_seconds": null},
                {"command": "move", "success": true, "elapsed_seconds": null},
                {"command": "speed", "success": true, "elapsed_seconds": null},
                {"command": "hotend-t2", "success": true, "elapsed_seconds": null},
                {"command": "select-t3", "success": true, "elapsed_seconds": null},
                {"command": "part-fan", "success": true, "elapsed_seconds": null},
                {"command": "aux-fan", "success": false, "elapsed_seconds": null},
                {"command": "chamber-fan", "success": true, "elapsed_seconds": null},
                {"command": "load", "success": true, "elapsed_seconds": null},
                {"command": "reread", "success": true, "elapsed_seconds": null},
                {"command": "drying", "success": false, "elapsed_seconds": null},
                {"command": "dwell", "success": true, "elapsed_seconds": 3},
            ],
            "paused_report": {
                "state": "PAUSE", "job_id": JOB_ID, "percent": 25,
                "file": format!("pandar/{JOB_ID}/benchy.gcode"), "layers": [3, 120],
                "remaining_minutes": 60,
            },
            "control_snapshot": {
                "serial": SERIAL,
                "model": "Snapmaker U1",
                "state": "PAUSE",
                "active_nozzle": "T3",
                "nozzles": [
                    ["T0", "25.0", "0.0"], ["T1", "25.5", "0.0"],
                    ["T2", "26.0", "220.0"], ["T3", "26.5", "0.0"],
                ],
                "diameters": ["0.4", "0.6", "0.4", "0.8"],
                "bed_celsius": "24.5",
                "chamber_light_on": true,
                "fans": [
                    ["hotend", 100], ["part_cooling", 0], ["chamber", 100],
                    ["hotend_second", 80], ["controller", 100],
                ],
            },
            "diagnose": {
                "overall": "ok",
                "checks": [
                    ["configured_printer", "ok"], ["moonraker_http", "ok"],
                    ["klippy_state", "ok"], ["websocket", "ok"], ["camera", "ok"],
                ],
            },
            "camera": {"frame_bytes": 64, "webcam_requests": [
                "snapshot", "webrtc", "stream", "snapshot", "webrtc", "stream", "stream",
            ]},
            "stop": true,
            "cancelled_report": {
                "state": "IDLE", "job_id": JOB_ID, "percent": 25,
                "file": format!("pandar/{JOB_ID}/benchy.gcode"), "layers": [3, 120],
                "remaining_minutes": 0,
            },
            "printer_requests": [
                format!("upload root=gcodes path=pandar/{JOB_ID}/benchy.gcode bytes={}", GCODE.len()),
                format!("print_start pandar/{JOB_ID}/benchy.gcode"),
                "print_pause",
                "gcode M140 S60",
                "gcode SET_LED LED=cavity_led RED=1 GREEN=1 BLUE=1 WHITE=1",
                "gcode G28 X Y",
                "gcode G91\nG1 Z1.5 F600\nG90",
                "gcode M220 S124",
                "gcode SET_HEATER_TEMPERATURE HEATER=extruder2 TARGET=220",
                "gcode T3",
                "gcode M106 S0",
                "gcode SET_FAN_SPEED FAN=cavity_fan SPEED=1",
                "gcode AUTO_FEEDING EXTRUDER=1 LOAD=1",
                "gcode FILAMENT_DT_UPDATE CHANNEL=0",
                "gcode G4 P4000",
                "gcode-done G4 P4000",
                "print_cancel",
            ],
        })
    );
}
