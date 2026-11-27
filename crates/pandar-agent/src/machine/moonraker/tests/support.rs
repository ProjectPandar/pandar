//! Shared harness for the Moonraker E2E tests: a real gateway and command
//! handlers wired to one fake printer, plus transcript helpers.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::{sync::mpsc, time::timeout};

use crate::{
    AgentConfig,
    commands::{ArtifactReader, handle_command_with_reader},
    machine::runtime::RuntimeBambuMachineGateway,
};
use pandar_protocol::agent::v1::{
    AgentEvent, HubCommand, LinkPrinter, PrintProjectFile, PrintProjectFileOptions,
    PrintSubmissionSource, PrinterOperation, agent_event, hub_command, printer_operation,
};

pub(super) const SERIAL: &str = "A1B2C3D4E5F60718";
pub(super) const JOB_ID: &str = "0190f7a2-5c1e-7d2b-9a40-3f6c2b8e1d57";
pub(super) const ARTIFACT_PATH: &str = "/api/v1/agents/agent-1/artifacts/benchy";
pub(super) const GCODE: &[u8] = b"; Snapmaker U1\nG28\nG1 X10 Y10\n";

pub(super) fn snapshot_summary(snapshot: &pandar_protocol::agent::v1::PrinterSnapshot) -> Value {
    json!({
        "serial": snapshot.serial,
        "model": snapshot.model,
        "state": snapshot.state,
        "active_nozzle": snapshot.active_nozzle,
        "nozzles": snapshot.nozzle_temperatures.iter()
            .map(|nozzle| json!([nozzle.label, nozzle.current_celsius, nozzle.target_celsius]))
            .collect::<Vec<_>>(),
        "diameters": snapshot.nozzle_temperatures.iter()
            .map(|nozzle| nozzle.diameter_mm.clone())
            .collect::<Vec<_>>(),
        "bed_celsius": snapshot.bed_temperature_celsius,
        "chamber_light_on": snapshot.chamber_light_on,
        "fans": snapshot.cooling_system.as_ref().map(|cooling| cooling.fans.iter()
            .map(|fan| json!([fan_kind(fan.kind), fan.speed_percent]))
            .collect::<Vec<_>>()),
    })
}

pub(super) fn fan_kind(kind: i32) -> String {
    use pandar_protocol::agent::v1::PrinterCoolingFanKind;
    let name = PrinterCoolingFanKind::try_from(kind).unwrap().as_str_name();
    name.strip_prefix("PRINTER_COOLING_FAN_KIND_")
        .unwrap()
        .to_ascii_lowercase()
}

pub(super) fn report_summary(report: &pandar_protocol::agent::v1::PrintJobReport) -> Value {
    json!({
        "state": report.gcode_state,
        "job_id": report.job_id,
        "percent": report.percent,
        "file": report.gcode_file,
        "layers": [report.current_layer, report.total_layers],
        "remaining_minutes": report.remaining_time_minutes,
    })
}

pub(super) fn write_artifact(file: &str, transcript: &Value) {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/e2e-artifacts");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join(file),
        serde_json::to_vec_pretty(transcript).unwrap(),
    )
    .unwrap();
}

/// A real gateway and command handlers wired to one printer.
pub(super) struct Harness {
    config: AgentConfig,
    pub(super) gateway: RuntimeBambuMachineGateway,
    sender: mpsc::Sender<AgentEvent>,
    events: EventLog,
}

impl Harness {
    pub(super) async fn start() -> Self {
        let config = test_config();
        let gateway =
            RuntimeBambuMachineGateway::new(config.clone(), Vec::new(), Duration::from_secs(5));
        let (sender, receiver) = mpsc::channel(256);
        Self {
            config,
            gateway,
            sender,
            events: EventLog::new(receiver),
        }
    }

    pub(super) async fn run(&self, command_id: &'static str, command: hub_command::Command) {
        handle_command_with_reader(
            &self.config,
            &self.gateway,
            &GcodeArtifact,
            &self.sender,
            HubCommand {
                command_id: command_id.to_owned(),
                command: Some(command),
            },
        )
        .await
        .unwrap();
    }

    /// Runs a command to completion and returns the Agent's result for it.
    pub(super) async fn dispatch(
        &mut self,
        command_id: &'static str,
        command: hub_command::Command,
    ) -> pandar_protocol::agent::v1::CommandResult {
        self.run(command_id, command).await;
        self.result(command_id).await
    }

    pub(super) async fn result(
        &mut self,
        command_id: &str,
    ) -> pandar_protocol::agent::v1::CommandResult {
        self.events.result(command_id).await
    }

    pub(super) async fn find<T>(
        &mut self,
        matcher: impl Fn(&agent_event::Event) -> Option<T>,
    ) -> T {
        self.events.find(matcher).await
    }

    pub(super) async fn find_within<T>(
        &mut self,
        duration: Duration,
        matcher: impl Fn(&agent_event::Event) -> Option<T>,
    ) -> Option<T> {
        self.events.find_within(duration, matcher).await
    }

    pub(super) async fn report(
        &mut self,
        predicate: impl Fn(&pandar_protocol::agent::v1::PrintJobReport) -> bool,
    ) -> pandar_protocol::agent::v1::PrintJobReport {
        self.events.report(predicate).await
    }
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

pub(super) fn link(host: &str, api_key: &str) -> hub_command::Command {
    hub_command::Command::LinkPrinter(LinkPrinter {
        host: host.to_owned(),
        access_code: api_key.to_owned(),
        name: "Workshop U1".to_owned(),
        printer_type: "Moonraker".to_owned(),
    })
}

pub(super) fn operate(operation: printer_operation::Operation) -> hub_command::Command {
    hub_command::Command::PrinterOperation(PrinterOperation {
        serial_number: SERIAL.to_owned(),
        required_device_features: Vec::new(),
        operation: Some(operation),
    })
}

pub(super) fn print_command() -> hub_command::Command {
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

    pub(super) async fn find<T>(
        &mut self,
        matcher: impl Fn(&agent_event::Event) -> Option<T>,
    ) -> T {
        self.find_within(Duration::from_secs(15), matcher)
            .await
            .expect("expected agent event did not arrive")
    }

    /// Waits up to `duration` for a matching event; `None` if none arrives.
    pub(super) async fn find_within<T>(
        &mut self,
        duration: Duration,
        matcher: impl Fn(&agent_event::Event) -> Option<T>,
    ) -> Option<T> {
        timeout(duration, async {
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
        .ok()
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
