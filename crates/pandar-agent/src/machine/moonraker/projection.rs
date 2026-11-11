use pandar_core::AmsUnitKind;

use super::{
    MoonrakerEndpoint,
    status::{
        DisplayStatus, GcodeMove, Heater, KlipperStatus, Led, ObjectSelection, PrintStats,
        PrintTaskConfig, Toolhead, VirtualSdcard, Webhooks, extruder_index,
    },
};
use crate::machine::{
    MachineNozzleTemperature, MachineSnapshot,
    materials::{
        AmsUnitPatch, MaterialFieldsPatch, MaterialPatchDocument, MaterialTrayEntryPatch,
        MaterialTrayPatch, ams_active_tray_patch, empty_tray_clear_patch,
    },
    mqtt::{MachineReportDiagnostic, MachineReportDiagnosticPayload, PrintReportProgress},
};

/// Directory under the Moonraker `gcodes` root that Pandar uploads into. Each
/// job gets `pandar/<job_id>/<filename>`, so the active `print_stats.filename`
/// identifies the Pandar job across Agent restarts.
pub(super) const UPLOAD_ROOT: &str = "pandar";

/// `print_task_config` filament channels are projected as one AMS-shaped unit so the
/// dashboard materials panel and slot operations apply unchanged.
pub(super) const MATERIAL_UNIT_ID: &str = "0";

/// Klipper speed factors matching Pandar's four speed levels.
const SPEED_LEVELS: [(u8, f64); 4] = [(1, 0.5), (2, 1.0), (3, 1.24), (4, 1.66)];

pub(super) fn speed_factor_percent(level: u8) -> Option<u32> {
    SPEED_LEVELS
        .iter()
        .find(|(candidate, _)| *candidate == level)
        .map(|(_, factor)| (factor * 100.0).round() as u32)
}

/// Translates Klipper state into the Bambu-style `gcode_state` vocabulary the
/// Hub job lifecycle and dashboard already understand.
pub(super) fn gcode_state(status: &KlipperStatus) -> &'static str {
    let webhooks: Webhooks = status.object("webhooks");
    match webhooks.state.as_str() {
        "shutdown" | "error" => return "FAILED",
        "startup" => return "PREPARE",
        _ => {}
    }
    let print_stats: PrintStats = status.object("print_stats");
    match print_stats.state.as_str() {
        "printing" => "RUNNING",
        "paused" => "PAUSE",
        "complete" => "FINISH",
        "error" => "FAILED",
        _ => "IDLE",
    }
}

pub(super) fn machine_snapshot(
    endpoint: &MoonrakerEndpoint,
    selection: &ObjectSelection,
    status: &KlipperStatus,
) -> MachineSnapshot {
    let multi_tool = selection.extruders.len() > 1;
    let nozzle_temperatures = selection
        .extruders
        .iter()
        .map(|name| {
            let heater: Heater = status.object(name);
            MachineNozzleTemperature {
                label: multi_tool
                    .then(|| extruder_index(name).map(tool_label))
                    .flatten(),
                current_celsius: heater.temperature.and_then(temperature_string),
                target_celsius: heater.target.and_then(temperature_string),
                diameter_mm: None,
                nozzle_type: None,
                snow: None,
                hnow: None,
            }
        })
        .collect();
    let toolhead: Toolhead = status.object("toolhead");
    let bed: Heater = status.object("heater_bed");
    let chamber: Option<Heater> = selection.chamber.as_deref().map(|name| status.object(name));
    let chamber_light_on = selection
        .light
        .as_deref()
        .filter(|name| status.has(name))
        .map(|name| {
            let led: Led = status.object(name);
            led.color_data
                .iter()
                .flatten()
                .any(|channel| *channel > 0.0)
        });

    MachineSnapshot {
        serial: endpoint.serial.clone(),
        host: Some(endpoint.host.clone()),
        access_code: endpoint.api_key.clone(),
        name: display_name(endpoint),
        model: endpoint.model.clone(),
        state: Some(gcode_state(status).to_owned()),
        nozzle_temperatures,
        active_nozzle: multi_tool
            .then(|| extruder_index(&toolhead.extruder).map(tool_label))
            .flatten(),
        bed_temperature_celsius: bed.temperature.and_then(temperature_string),
        bed_target_temperature_celsius: bed.target.and_then(temperature_string),
        chamber_temperature_celsius: chamber
            .as_ref()
            .and_then(|heater| heater.temperature)
            .and_then(temperature_string),
        chamber_target_temperature_celsius: chamber
            .as_ref()
            .and_then(|heater| heater.target)
            .and_then(temperature_string),
        chamber_light_on,
        cooling_system: None,
        device_features: None,
        device_features2: None,
        nozzle_system: None,
        telemetry_authoritative: true,
        printer_family: pandar_core::PrinterFamily::Moonraker,
    }
}

/// Identity-only snapshot used before live status arrives (`state: None`) and
/// while the printer is unreachable (`state: "offline"`).
pub(super) fn endpoint_snapshot(
    endpoint: &MoonrakerEndpoint,
    state: Option<&str>,
) -> MachineSnapshot {
    MachineSnapshot {
        serial: endpoint.serial.clone(),
        host: Some(endpoint.host.clone()),
        access_code: endpoint.api_key.clone(),
        name: display_name(endpoint),
        model: endpoint.model.clone(),
        state: state.map(str::to_owned),
        nozzle_temperatures: Vec::new(),
        active_nozzle: None,
        bed_temperature_celsius: None,
        bed_target_temperature_celsius: None,
        chamber_temperature_celsius: None,
        chamber_target_temperature_celsius: None,
        chamber_light_on: None,
        cooling_system: None,
        device_features: None,
        device_features2: None,
        nozzle_system: None,
        telemetry_authoritative: false,
        printer_family: pandar_core::PrinterFamily::Moonraker,
    }
}

pub(super) fn display_name(endpoint: &MoonrakerEndpoint) -> String {
    endpoint
        .name
        .clone()
        .unwrap_or_else(|| endpoint.serial.clone())
}

pub(super) fn print_report(
    endpoint: &MoonrakerEndpoint,
    status: &KlipperStatus,
    observed_at: String,
    printer_materials_json: String,
) -> PrintReportProgress {
    let print_stats: PrintStats = status.object("print_stats");
    let sdcard: VirtualSdcard = status.object("virtual_sdcard");
    let display: DisplayStatus = status.object("display_status");
    let gcode_move: GcodeMove = status.object("gcode_move");
    let filename = non_blank(&print_stats.filename);
    let job_id = filename.as_deref().and_then(job_id_from_path);
    let progress = display.progress.unwrap_or(sdcard.progress).clamp(0.0, 1.0);
    let active = matches!(print_stats.state.as_str(), "printing" | "paused");
    let remaining_time_minutes = (active && progress > 0.01).then(|| {
        let remaining_seconds = print_stats.print_duration / progress - print_stats.print_duration;
        ((remaining_seconds / 60.0).round().max(0.0) as u32).min(4320)
    });
    let mut diagnostics = Vec::new();
    if print_stats.state == "error" && !print_stats.message.trim().is_empty() {
        diagnostics.push(MachineReportDiagnostic {
            kind: "print_error".to_owned(),
            severity: "error".to_owned(),
            code: None,
            message: print_stats.message.trim().to_owned(),
            payload: MachineReportDiagnosticPayload::Null,
        });
    }
    let webhooks: Webhooks = status.object("webhooks");
    if matches!(webhooks.state.as_str(), "shutdown" | "error") {
        diagnostics.push(MachineReportDiagnostic {
            kind: "klippy_state".to_owned(),
            severity: "error".to_owned(),
            code: Some(webhooks.state.clone()),
            message: webhooks.state_message.trim().to_owned(),
            payload: MachineReportDiagnosticPayload::Null,
        });
    }

    PrintReportProgress {
        serial: endpoint.serial.clone(),
        job_id: job_id.clone(),
        job_attr: None,
        print_error: None,
        printer_job_id: None,
        artifact_id: None,
        subtask_id: None,
        gcode_state: Some(gcode_state(status).to_owned()),
        percent: Some((progress * 100.0).round() as u8),
        speed_level: gcode_move.speed_factor.and_then(speed_level),
        remaining_time_minutes,
        current_layer: print_stats.info.current_layer,
        total_layers: print_stats.info.total_layer,
        subtask_name: filename.as_deref().map(file_stem),
        gcode_file: filename,
        hms: None,
        diagnostics,
        observed_at,
        printer_materials_json,
    }
}

/// Extracts `<job_id>` from `pandar/<job_id>/<filename>`.
pub(super) fn job_id_from_path(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    (parts.next() == Some(UPLOAD_ROOT))
        .then(|| parts.next())
        .flatten()
        .filter(|job_id| !job_id.is_empty() && parts.next().is_some())
        .map(str::to_owned)
}

/// Projects `print_task_config` filament channels into the material patch document.
pub(super) fn materials_json(
    selection: &ObjectSelection,
    status: &KlipperStatus,
    observed_at: &str,
) -> Option<String> {
    if !selection.filament_channels || !status.has("print_task_config") {
        return None;
    }
    let config: PrintTaskConfig = status.object("print_task_config");
    let trays = (0..config.filament_exist.len())
        .map(|channel| {
            let tray_id = channel.to_string();
            let global_tray_id = Some(channel as u64);
            if !config.filament_exist.get(channel).copied().unwrap_or(false) {
                return empty_tray_clear_patch(tray_id, AmsUnitKind::Ams, global_tray_id);
            }
            let text = |values: &[String]| values.get(channel).and_then(|value| non_blank(value));
            let vendor = text(&config.filament_vendor);
            let filament_type = text(&config.filament_type);
            let sub_type = text(&config.filament_sub_type);
            let name = [vendor, filament_type.clone(), sub_type]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            MaterialTrayPatch::Present(MaterialTrayEntryPatch {
                tray_id,
                exists: true,
                unit_kind: Some(AmsUnitKind::Ams),
                global_tray_id,
                fields: MaterialFieldsPatch {
                    filament_type,
                    color: text(&config.filament_color_rgba)
                        .map(|color| color.to_ascii_uppercase()),
                    name: non_blank(&name),
                    toolhead: Some(channel.to_string()),
                    ..MaterialFieldsPatch::default()
                },
            })
        })
        .collect();
    let toolhead: Toolhead = status.object("toolhead");
    let document = MaterialPatchDocument {
        document_type: "printer_material_patch",
        observed_at,
        cfg: None,
        aux: None,
        stat: None,
        filament_switch_installed: None,
        ams_units: vec![AmsUnitPatch {
            unit_id: MATERIAL_UNIT_ID.to_owned(),
            unit_kind: Some(AmsUnitKind::Ams),
            info: None,
            humidity: None,
            humidity_level: None,
            temperature_celsius: None,
            dry_status: None,
            dry_time_minutes: None,
            toolhead: None,
            trays,
            replace_trays: true,
        }],
        external_spools: None,
        replace_external_spools: false,
        active_tray: extruder_index(&toolhead.extruder).map(|index| {
            ams_active_tray_patch(
                i64::from(index),
                MATERIAL_UNIT_ID.to_owned(),
                index.to_string(),
            )
        }),
    };
    Some(serde_json::to_string(&document).expect("material patch document is serializable"))
}

fn tool_label(index: u32) -> String {
    format!("T{index}")
}

fn speed_level(factor: f64) -> Option<u8> {
    SPEED_LEVELS
        .iter()
        .find(|(_, level_factor)| (factor - level_factor).abs() < 0.005)
        .map(|(level, _)| *level)
}

fn temperature_string(value: f64) -> Option<String> {
    (value.is_finite() && value >= 0.0).then(|| format!("{value:.1}"))
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".gcode").unwrap_or(name).to_owned()
}

fn non_blank(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
