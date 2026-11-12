use anyhow::Context;
use pandar_core::{BambuDeviceFeatures, compatibility::normalize_model};

use super::{PrinterOperation, axis};
use crate::machine::mqtt::{
    AmsDryingCommand, AmsFilamentCommand, AmsSlotCommand, BambuMqttCommand, GcodeLineCommand,
    HandlePrintErrorCommand, PrintSpeed, SetFanSpeedCommand, SetNozzleTemperatureCommand,
};

pub(in crate::machine) fn mqtt_command_for_printer_operation(
    operation: PrinterOperation,
) -> anyhow::Result<BambuMqttCommand> {
    mqtt_command_for_printer_operation_with_features(operation, None, None)
}

pub(super) fn mqtt_command_for_printer_operation_with_features(
    operation: PrinterOperation,
    observed_features: Option<BambuDeviceFeatures>,
    model: Option<&str>,
) -> anyhow::Result<BambuMqttCommand> {
    operation.validate().map_err(anyhow::Error::new)?;
    if model.and_then(normalize_model).as_deref() == Some("H3C") {
        match operation {
            PrinterOperation::AmsLoadFilament {
                ams_id,
                slot_id,
                external_id,
                ..
            } => return h3c_load_filament(ams_id, slot_id, external_id),
            PrinterOperation::AmsUnloadFilament {
                ams_id,
                slot_id,
                external_id,
                ..
            } => return h3c_unload_filament(ams_id, slot_id, external_id),
            _ => {}
        }
    }
    match operation {
        PrinterOperation::Pause {} => Ok(BambuMqttCommand::PausePrint),
        PrinterOperation::Resume {} => Ok(BambuMqttCommand::ResumePrint),
        PrinterOperation::Stop {} => Ok(BambuMqttCommand::StopPrint),
        PrinterOperation::HandlePrintError {
            error_action,
            print_error,
            printer_job_id,
            sequence_id,
        } => Ok(BambuMqttCommand::HandlePrintError(
            HandlePrintErrorCommand {
                error_action,
                print_error,
                printer_job_id,
                sequence_id,
            },
        )),
        PrinterOperation::ToggleLight {} => {
            unreachable!("toggle_light is handled before payload mapping")
        }
        PrinterOperation::SetChamberLight { .. } => {
            unreachable!("set_chamber_light is handled before payload mapping")
        }
        PrinterOperation::SetPrintSpeed { speed_mode } => Ok(BambuMqttCommand::SetPrintSpeed(
            PrintSpeed::new(speed_mode)?,
        )),
        PrinterOperation::SetFanSpeed {
            fan_index,
            speed_percent,
            airduct,
        } => {
            if airduct {
                Ok(BambuMqttCommand::SetFanSpeed(SetFanSpeedCommand {
                    fan_index,
                    speed_percent,
                }))
            } else {
                let pwm = (u16::from(speed_percent) * 255 + 50) / 100;
                Ok(BambuMqttCommand::GcodeLine(GcodeLineCommand {
                    param: format!("M106 P{fan_index} S{pwm}"),
                }))
            }
        }
        PrinterOperation::GcodeLine { param } => {
            Ok(BambuMqttCommand::GcodeLine(GcodeLineCommand { param }))
        }
        PrinterOperation::GetAutoNozzleMapping { request } => {
            Ok(BambuMqttCommand::GetAutoNozzleMapping(request))
        }
        PrinterOperation::NozzleHolderCtrl { action } => {
            Ok(BambuMqttCommand::NozzleHolderCtrl(action))
        }
        PrinterOperation::NozzleInfoConfirm { id } => Ok(BambuMqttCommand::NozzleInfoConfirm(id)),
        PrinterOperation::HolderNozzleRefresh { id } => {
            Ok(BambuMqttCommand::HolderNozzleRefresh(id))
        }
        PrinterOperation::SelectExtruder { extruder_id } => {
            Ok(BambuMqttCommand::SelectExtruder(extruder_id))
        }
        PrinterOperation::Home {
            axes,
            required_device_features,
        } => axis::home_command(
            axes,
            required_device_features
                .first()
                .copied()
                .map(pandar_core::RequiredDeviceFeature::bambu_feature),
            observed_features,
        ),
        PrinterOperation::MoveAxes {
            movements,
            feedrate_mm_per_min,
            required_device_features,
        } => {
            let (x_mm, y_mm, z_mm) = movement_axes(&movements);
            axis::move_axes_command(
                x_mm,
                y_mm,
                z_mm,
                feedrate_mm_per_min.map(f64::from),
                required_device_features
                    .first()
                    .copied()
                    .map(pandar_core::RequiredDeviceFeature::bambu_feature),
                observed_features,
            )
        }
        PrinterOperation::SetHotendTemperature {
            temperature_celsius,
            wait,
            extruder_id,
        } => match extruder_id {
            Some(extruder_id) => Ok(BambuMqttCommand::SetNozzleTemperature(
                SetNozzleTemperatureCommand {
                    extruder_id,
                    target_temp: temperature_celsius,
                },
            )),
            None => Ok(BambuMqttCommand::GcodeLine(GcodeLineCommand {
                param: format!(
                    "{} S{}",
                    if wait { "M109" } else { "M104" },
                    temperature_celsius
                ),
            })),
        },
        PrinterOperation::SetBedTemperature {
            temperature_celsius,
            wait,
        } => Ok(BambuMqttCommand::GcodeLine(GcodeLineCommand {
            param: format!(
                "{} S{}",
                if wait { "M190" } else { "M140" },
                temperature_celsius
            ),
        })),
        PrinterOperation::SetChamberTemperature {
            temperature_celsius,
            wait,
        } => Ok(BambuMqttCommand::GcodeLine(GcodeLineCommand {
            param: if wait {
                [
                    "M106 P2 S255".to_string(),
                    format!("M191 S{}", temperature_celsius),
                    "M106 P2 S0".to_string(),
                ]
                .join("\n")
            } else {
                format!("M141 S{}", temperature_celsius)
            },
        })),
        PrinterOperation::AmsRereadRfid { ams_id, slot_id } => {
            Ok(BambuMqttCommand::AmsRereadRfid(AmsSlotCommand {
                ams_id,
                slot_id,
            }))
        }
        PrinterOperation::AmsLoadFilament {
            ams_id,
            slot_id,
            global_tray_id,
            external_id,
            extruder_id,
        } => Ok(BambuMqttCommand::AmsLoadFilament(AmsFilamentCommand {
            ams_id: ams_command_ams_id(ams_id, external_id.as_deref()),
            slot_id: ams_command_slot_id(slot_id, external_id.as_deref()),
            target: global_tray_id.unwrap_or(slot_id),
            extruder_id,
        })),
        PrinterOperation::AmsUnloadFilament {
            ams_id,
            external_id,
            ..
        } => Ok(BambuMqttCommand::AmsUnloadFilament(AmsFilamentCommand {
            ams_id: ams_command_ams_id(ams_id, external_id.as_deref()),
            slot_id: 255,
            target: 255,
            extruder_id: None,
        })),
        PrinterOperation::AmsStartDrying {
            ams_id,
            temperature_celsius,
            duration_hours,
            filament,
            rotate_tray,
        } => Ok(BambuMqttCommand::AmsStartDrying(AmsDryingCommand {
            ams_id,
            temperature_celsius,
            duration_hours,
            filament,
            rotate_tray,
        })),
        PrinterOperation::AmsStopDrying { ams_id } => Ok(BambuMqttCommand::AmsStopDrying(ams_id)),
    }
}

fn movement_axes(
    movements: &[pandar_core::PrinterAxisMovement],
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let mut axes = (None, None, None);
    for movement in movements {
        match movement.axis {
            pandar_core::PrinterAxis::X => axes.0 = Some(movement.delta_mm),
            pandar_core::PrinterAxis::Y => axes.1 = Some(movement.delta_mm),
            pandar_core::PrinterAxis::Z => axes.2 = Some(movement.delta_mm),
        }
    }
    axes
}

// The H3C feeds each of its nine hotends from its own AMS 3H slot or external
// input, so filament commands name the hotend the way Studio's rack filament
// page does (PageFilament.cpp), always with `extruder_id` 0.
fn h3c_load_filament(
    ams_id: u32,
    slot_id: u32,
    external_id: Option<String>,
) -> anyhow::Result<BambuMqttCommand> {
    // Studio's CanAutoFeed: an external input has no feeder motor of its own.
    if let Some(external_id) = external_id {
        anyhow::bail!(
            "H3C external input {external_id} cannot feed filament by itself; push it into the hotend by hand"
        );
    }
    Ok(BambuMqttCommand::AmsLoadFilament(AmsFilamentCommand {
        ams_id,
        slot_id,
        target: if ams_id < 16 {
            ams_id * 4 + slot_id
        } else {
            ams_id
        },
        extruder_id: Some(0),
    }))
}

fn h3c_unload_filament(
    ams_id: u32,
    slot_id: u32,
    external_id: Option<String>,
) -> anyhow::Result<BambuMqttCommand> {
    let (ams_id, slot_id) = match external_id {
        Some(external_id) => (
            external_id
                .parse()
                .with_context(|| format!("invalid H3C external input id {external_id:?}"))?,
            255,
        ),
        // 255 is Studio's own unaddressed or external-input unload, already encoded.
        None if slot_id == 255 => (ams_id, 255),
        None => (ams_id, 255 - slot_id),
    };
    Ok(BambuMqttCommand::AmsUnloadFilament(AmsFilamentCommand {
        ams_id,
        slot_id,
        target: 255,
        extruder_id: Some(0),
    }))
}

fn ams_command_ams_id(ams_id: u32, external_id: Option<&str>) -> u32 {
    if external_id.is_some() { 255 } else { ams_id }
}

fn ams_command_slot_id(slot_id: u32, external_id: Option<&str>) -> u32 {
    if matches!(external_id, Some("254")) {
        254
    } else {
        slot_id
    }
}
