//! End-to-end: Hub commands drive the real Agent command handlers and runtime
//! gateway against an in-process Moonraker modelled on a Snapmaker U1 target.
//! The run writes `target/e2e-artifacts/moonraker-agent.json`, a transcript
//! of everything the printer received and the Agent reported.

mod fake_moonraker;

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::{sync::mpsc, time::timeout};

use fake_moonraker::{API_KEY, CPU_SERIAL, FakeMoonraker, JPEG_FRAME};

use crate::{
    AgentConfig,
    commands::{ArtifactReader, handle_command_with_reader},
    machine::{BambuMachineGateway, CameraSource, runtime::RuntimeBambuMachineGateway},
};
use pandar_protocol::agent::v1::{
    AgentEvent, AmsStartDryingOperation, Axis, AxisMovement, HomeOperation, HubCommand,
    LinkPrinter, MoveAxesOperation, PauseOperation, PrintProjectFile, PrintProjectFileOptions,
    PrintSubmissionSource, PrinterFamily, PrinterOperation, SetBedTemperatureOperation,
    SetChamberLightOperation, SetPrintSpeedOperation, StopOperation, agent_event, hub_command,
    printer_operation,
};

const SERIAL: &str = "A1B2C3D4E5F60718";
const JOB_ID: &str = "0190f7a2-5c1e-7d2b-9a40-3f6c2b8e1d57";
const ARTIFACT_PATH: &str = "/api/v1/agents/agent-1/artifacts/benchy";
const GCODE: &[u8] = b"; Snapmaker U1\nG28\nG1 X10 Y10\n";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moonraker_link_report_print_control_and_camera() {
    assert_eq!(CPU_SERIAL.to_ascii_uppercase(), SERIAL);
    let printer = FakeMoonraker::start().await;
    let config = test_config();
    let gateway =
        RuntimeBambuMachineGateway::new(config.clone(), Vec::new(), Duration::from_secs(5));
    let (sender, receiver) = mpsc::channel(256);
    let mut events = EventLog::new(receiver);
    let run = |command_id: &'static str, command: hub_command::Command| {
        let (config, gateway, sender) = (&config, &gateway, &sender);
        async move {
            handle_command_with_reader(
                config,
                gateway,
                &GcodeArtifact,
                sender,
                HubCommand {
                    command_id: command_id.to_owned(),
                    command: Some(command),
                },
            )
            .await
            .unwrap();
        }
    };

    run("link-wrong-key", link(&printer.host, "wrong-key")).await;
    let rejected = events.result("link-wrong-key").await;
    assert!(!rejected.error.contains("wrong-key"), "{}", rejected.error);

    run("link", link(&printer.host, API_KEY)).await;
    let linked = events.result("link").await;
    let linked_snapshot = events
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
    let materials = events
        .find(|event| match event {
            agent_event::Event::PrinterMaterialsSnapshot(materials) => {
                Some(serde_json::from_str::<Value>(&materials.printer_materials_json).unwrap())
            }
            _ => None,
        })
        .await;
    let idle = events.report(|report| report.gcode_state == "IDLE").await;

    run("print", print_command()).await;
    let printed = events.result("print").await;
    let running = events
        .report(|report| report.gcode_state == "RUNNING" && report.job_id == JOB_ID)
        .await;

    let mut operations = Vec::new();
    for (command_id, operation) in [
        (
            "pause",
            printer_operation::Operation::Pause(PauseOperation {}),
        ),
        (
            "bed",
            printer_operation::Operation::SetBedTemperature(SetBedTemperatureOperation {
                temperature_celsius: 60,
                wait: false,
            }),
        ),
        (
            "light",
            printer_operation::Operation::SetChamberLight(SetChamberLightOperation { on: true }),
        ),
        (
            "home",
            printer_operation::Operation::Home(HomeOperation {
                axes: vec![Axis::X as i32, Axis::Y as i32],
            }),
        ),
        (
            "move",
            printer_operation::Operation::MoveAxes(MoveAxesOperation {
                movements: vec![AxisMovement {
                    axis: Axis::Z as i32,
                    delta_mm: 1.5,
                }],
                feedrate_mm_per_min: 600,
            }),
        ),
        (
            "speed",
            printer_operation::Operation::SetPrintSpeed(SetPrintSpeedOperation { speed_mode: 3 }),
        ),
        (
            "drying",
            printer_operation::Operation::AmsStartDrying(AmsStartDryingOperation {
                ams_id: 0,
                temperature_celsius: 55,
                duration_hours: 4,
                filament: "PLA".to_owned(),
                rotate_tray: false,
            }),
        ),
    ] {
        run(command_id, operate(operation)).await;
        let result = events.result(command_id).await;
        operations.push(json!({"command": command_id, "success": result.success}));
    }
    let paused = events
        .report(|report| report.gcode_state == "PAUSE" && report.job_id == JOB_ID)
        .await;
    let light_on = events
        .find(|event| match event {
            agent_event::Event::PrinterSnapshot(snapshot)
                if snapshot.chamber_light_on == Some(true) =>
            {
                Some(true)
            }
            _ => None,
        })
        .await;

    let camera = match gateway.camera_source(SERIAL).await.unwrap() {
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

    run(
        "stop",
        operate(printer_operation::Operation::Stop(StopOperation {})),
    )
    .await;
    let stopped = events.result("stop").await;
    let cancelled = events
        .report(|report| report.gcode_state == "IDLE" && report.job_id == JOB_ID)
        .await;

    let transcript = json!({
        "link_rejected": {
            "success": rejected.success,
            "result": serde_json::from_str::<Value>(&rejected.result_json).unwrap(),
        },
        "link": serde_json::from_str::<Value>(&linked.result_json).unwrap(),
        "linked_snapshot": {
            "serial": linked_snapshot.serial,
            "model": linked_snapshot.model,
            "state": linked_snapshot.state,
            "active_nozzle": linked_snapshot.active_nozzle,
            "nozzles": linked_snapshot.nozzle_temperatures.iter()
                .map(|nozzle| json!([nozzle.label, nozzle.current_celsius]))
                .collect::<Vec<_>>(),
            "bed_celsius": linked_snapshot.bed_temperature_celsius,
            "chamber_light_on": linked_snapshot.chamber_light_on,
        },
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
        "light_on_snapshot": light_on,
        "camera_frame_bytes": frame.len(),
        "stop": stopped.success,
        "cancelled_report": report_summary(&cancelled),
        "printer_requests": printer.requests().await,
    });
    write_artifact(&transcript);

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
                "model": "Klipper",
                "status": "IDLE",
            },
            "linked_snapshot": {
                "serial": SERIAL,
                "model": "Klipper",
                "state": "IDLE",
                "active_nozzle": "T0",
                "nozzles": [["T0", "25.0"], ["T1", "25.5"], ["T2", "26.0"], ["T3", "26.5"]],
                "bed_celsius": "24.5",
                "chamber_light_on": false,
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
            "idle_report": {"state": "IDLE", "job_id": "", "percent": 0, "file": "", "layers": [0, 0]},
            "print": format!("pandar/{JOB_ID}/benchy.gcode"),
            "running_report": {
                "state": "RUNNING", "job_id": JOB_ID, "percent": 25,
                "file": format!("pandar/{JOB_ID}/benchy.gcode"), "layers": [3, 120],
            },
            "operations": [
                {"command": "pause", "success": true},
                {"command": "bed", "success": true},
                {"command": "light", "success": true},
                {"command": "home", "success": true},
                {"command": "move", "success": true},
                {"command": "speed", "success": true},
                {"command": "drying", "success": false},
            ],
            "paused_report": {
                "state": "PAUSE", "job_id": JOB_ID, "percent": 25,
                "file": format!("pandar/{JOB_ID}/benchy.gcode"), "layers": [3, 120],
            },
            "light_on_snapshot": true,
            "camera_frame_bytes": 64,
            "stop": true,
            "cancelled_report": {
                "state": "IDLE", "job_id": JOB_ID, "percent": 25,
                "file": format!("pandar/{JOB_ID}/benchy.gcode"), "layers": [3, 120],
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
                "print_cancel",
            ],
        })
    );
}

fn report_summary(report: &pandar_protocol::agent::v1::PrintJobReport) -> Value {
    json!({
        "state": report.gcode_state,
        "job_id": report.job_id,
        "percent": report.percent,
        "file": report.gcode_file,
        "layers": [report.current_layer, report.total_layers],
    })
}

fn write_artifact(transcript: &Value) {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/e2e-artifacts");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("moonraker-agent.json"),
        serde_json::to_vec_pretty(transcript).unwrap(),
    )
    .unwrap();
}

fn test_config() -> AgentConfig {
    AgentConfig {
        hub_grpc_url: "http://hub.internal:50051".to_owned(),
        hub_api_url: None,
        agent_name: "garage".to_owned(),
        agent_id: "agent-1".to_owned(),
        tenant_id: "tenant-1".to_owned(),
        agent_credential: "pandar_ac_test".to_owned(),
        agent_version: "9.8.7".to_owned(),
        printers: "[]".to_owned(),
    }
}

fn link(host: &str, api_key: &str) -> hub_command::Command {
    hub_command::Command::LinkPrinter(LinkPrinter {
        host: host.to_owned(),
        access_code: api_key.to_owned(),
        name: "Workshop U1".to_owned(),
        printer_type: "Moonraker".to_owned(),
    })
}

fn operate(operation: printer_operation::Operation) -> hub_command::Command {
    hub_command::Command::PrinterOperation(PrinterOperation {
        serial_number: SERIAL.to_owned(),
        required_device_features: Vec::new(),
        operation: Some(operation),
    })
}

fn print_command() -> hub_command::Command {
    hub_command::Command::PrintProjectFile(PrintProjectFile {
        job_id: JOB_ID.to_owned(),
        artifact_id: "artifact-benchy".to_owned(),
        printer_id: "printer-1".to_owned(),
        serial_number: SERIAL.to_owned(),
        filename: "benchy.gcode".to_owned(),
        storage_path: "artifacts/benchy.gcode".to_owned(),
        size_bytes: GCODE.len() as u64,
        plate_id: 1,
        artifact_download_path: ARTIFACT_PATH.to_owned(),
        studio_submission_id: 1,
        options: Some(PrintProjectFileOptions {
            auto_bed_leveling: Some(0),
            auto_flow_cali: Some(0),
            auto_offset_cali: Some(0),
            ..PrintProjectFileOptions::default()
        }),
        task_metadata: None,
        submission_source: PrintSubmissionSource::Web as i32,
    })
}

struct GcodeArtifact;

#[async_trait]
impl ArtifactReader for GcodeArtifact {
    async fn read_artifact(&self, artifact_download_path: &str) -> anyhow::Result<Vec<u8>> {
        assert_eq!(artifact_download_path, ARTIFACT_PATH);
        Ok(GCODE.to_vec())
    }
}

/// Agent events in arrival order. Report forwarding races command results, so
/// lookups scan everything received so far and consume only the match.
struct EventLog {
    receiver: mpsc::Receiver<AgentEvent>,
    pending: Vec<agent_event::Event>,
}

impl EventLog {
    fn new(receiver: mpsc::Receiver<AgentEvent>) -> Self {
        Self {
            receiver,
            pending: Vec::new(),
        }
    }

    async fn find<T>(&mut self, matcher: impl Fn(&agent_event::Event) -> Option<T>) -> T {
        timeout(Duration::from_secs(15), async {
            let mut scanned = 0;
            loop {
                if let Some((index, found)) = self.pending[scanned..]
                    .iter()
                    .enumerate()
                    .find_map(|(index, event)| matcher(event).map(|found| (scanned + index, found)))
                {
                    self.pending.remove(index);
                    return found;
                }
                scanned = self.pending.len();
                let event = self
                    .receiver
                    .recv()
                    .await
                    .expect("agent event channel open");
                self.pending.push(event.event.expect("agent event payload"));
            }
        })
        .await
        .expect("expected agent event did not arrive")
    }

    async fn result(&mut self, command_id: &str) -> pandar_protocol::agent::v1::CommandResult {
        self.find(|event| match event {
            agent_event::Event::CommandResult(result) if result.command_id == command_id => {
                Some(result.clone())
            }
            _ => None,
        })
        .await
    }

    async fn report(
        &mut self,
        predicate: impl Fn(&pandar_protocol::agent::v1::PrintJobReport) -> bool,
    ) -> pandar_protocol::agent::v1::PrintJobReport {
        self.find(|event| match event {
            agent_event::Event::PrintJobReport(report) if predicate(report) => Some(report.clone()),
            _ => None,
        })
        .await
    }
}
