use anyhow::{Context, bail};
use pandar_core::{PrinterAxis, PrinterOperation};

use super::{
    MoonrakerClient,
    client::PrintAction,
    projection::speed_factor_percent,
    status::{KlipperStatus, Led, ObjectSelection, extruder_index},
};

const DEFAULT_MOVE_FEEDRATE_MM_PER_MIN: u32 = 3000;

pub(super) async fn operate(
    client: &MoonrakerClient,
    selection: &ObjectSelection,
    operation: PrinterOperation,
) -> anyhow::Result<()> {
    let action = operation.action();
    if let Some(command) = material_command(&operation) {
        if !selection.filament_channels {
            bail!("printer operation {action} is not supported on Moonraker printers");
        }
        ensure_firmware_supports(client, selection, command, action).await?;
    }
    let script = match operation {
        PrinterOperation::Pause {} => return client.print_action(PrintAction::Pause).await,
        PrinterOperation::Resume {} => return client.print_action(PrintAction::Resume).await,
        PrinterOperation::Stop {} => return client.print_action(PrintAction::Cancel).await,
        PrinterOperation::GcodeLine { param } => param,
        PrinterOperation::ToggleLight {} => {
            let on = !light_is_on(client, selection).await?;
            light_script(selection, on)?
        }
        PrinterOperation::SetChamberLight { on } => light_script(selection, on)?,
        PrinterOperation::SetPrintSpeed { speed_mode } => {
            let percent = speed_factor_percent(speed_mode)
                .with_context(|| format!("unsupported print speed level {speed_mode}"))?;
            format!("M220 S{percent}")
        }
        PrinterOperation::SetFanSpeed {
            fan_index,
            speed_percent,
            ..
        } => selection
            .fans
            .iter()
            .find(|fan| fan.control_index() == Some(fan_index))
            .with_context(|| format!("printer has no controllable fan {fan_index}"))?
            .speed_script(speed_percent),
        PrinterOperation::SelectExtruder { extruder_id } => format!("T{extruder_id}"),
        PrinterOperation::Home { axes, .. } => home_script(&axes),
        PrinterOperation::MoveAxes {
            movements,
            feedrate_mm_per_min,
            ..
        } => {
            let axes = movements
                .iter()
                .map(|movement| format!("{}{}", axis_letter(movement.axis), movement.delta_mm))
                .collect::<Vec<_>>()
                .join(" ");
            let feedrate = feedrate_mm_per_min.unwrap_or(DEFAULT_MOVE_FEEDRATE_MM_PER_MIN);
            format!("G91\nG1 {axes} F{feedrate}\nG90")
        }
        // Heating commands only set targets: Klipper's blocking `M109`/`M190`/`TEMPERATURE_WAIT`
        // would stall the G-code queue for minutes, so the Hub follows progress in telemetry.
        PrinterOperation::SetHotendTemperature {
            temperature_celsius,
            extruder_id,
            ..
        } => match extruder_id {
            // Without a tool the active extruder is heated, which is what `M104` does natively.
            None => format!("M104 S{temperature_celsius}"),
            // `M104 T<n>` is remapped by some firmware; the Klipper command addresses the heater directly.
            Some(index) => {
                let heater = extruder_name(selection, index)?;
                format!("SET_HEATER_TEMPERATURE HEATER={heater} TARGET={temperature_celsius}")
            }
        },
        PrinterOperation::SetBedTemperature {
            temperature_celsius,
            ..
        } => format!("M140 S{temperature_celsius}"),
        PrinterOperation::SetChamberTemperature {
            temperature_celsius,
            ..
        } => {
            let heater = selection
                .chamber
                .as_deref()
                .and_then(|name| name.strip_prefix("heater_generic "))
                .context("printer has no controllable chamber heater")?;
            format!("SET_HEATER_TEMPERATURE HEATER={heater} TARGET={temperature_celsius}")
        }
        PrinterOperation::AmsLoadFilament { slot_id, .. } => {
            format!("AUTO_FEEDING EXTRUDER={slot_id} LOAD=1")
        }
        PrinterOperation::AmsUnloadFilament { slot_id, .. } => {
            format!("AUTO_FEEDING EXTRUDER={slot_id} UNLOAD=1")
        }
        PrinterOperation::AmsRereadRfid { slot_id, .. } => {
            format!("FILAMENT_DT_UPDATE CHANNEL={slot_id}")
        }
        _ => bail!("printer operation {action} is not supported on Moonraker printers"),
    };
    client
        .run_gcode(&script)
        .await
        .with_context(|| format!("run Moonraker {action} G-code"))
}

/// The firmware-extension G-code a filament channel operation depends on.
fn material_command(operation: &PrinterOperation) -> Option<&'static str> {
    match operation {
        PrinterOperation::AmsLoadFilament { .. } | PrinterOperation::AmsUnloadFilament { .. } => {
            Some("AUTO_FEEDING")
        }
        PrinterOperation::AmsRereadRfid { .. } => Some("FILAMENT_DT_UPDATE"),
        _ => None,
    }
}

/// Firmware extensions register their commands dynamically, and only described
/// commands reach the G-code help dictionary (Snapmaker's `FILAMENT_DT_UPDATE`
/// has no description), so presence is any of: the help entry, a `gcode_macro`
/// section, or the extension's status object.
async fn ensure_firmware_supports(
    client: &MoonrakerClient,
    selection: &ObjectSelection,
    command: &str,
    action: &str,
) -> anyhow::Result<()> {
    let object = match command {
        "AUTO_FEEDING" => "gcode_macro AUTO_FEEDING",
        "FILAMENT_DT_UPDATE" => "filament_detect",
        _ => unreachable!("material commands are known"),
    };
    if !selection.has_object(object)
        && !client
            .gcode_help()
            .await
            .context("list Moonraker G-code commands")?
            .contains_key(command)
    {
        bail!("printer operation {action} is not supported: firmware has no {command} command");
    }
    Ok(())
}

fn extruder_name(selection: &ObjectSelection, index: u32) -> anyhow::Result<&str> {
    selection
        .extruders
        .iter()
        .map(String::as_str)
        .find(|name| extruder_index(name) == Some(index))
        .with_context(|| format!("printer has no extruder {index}"))
}

fn home_script(axes: &[PrinterAxis]) -> String {
    if axes.is_empty() {
        return "G28".to_owned();
    }
    let axes = axes
        .iter()
        .map(|axis| axis_letter(*axis))
        .collect::<Vec<_>>()
        .join(" ");
    format!("G28 {axes}")
}

fn axis_letter(axis: PrinterAxis) -> &'static str {
    match axis {
        PrinterAxis::X => "X",
        PrinterAxis::Y => "Y",
        PrinterAxis::Z => "Z",
    }
}

fn light_name(selection: &ObjectSelection) -> anyhow::Result<&str> {
    let object = selection
        .light
        .as_deref()
        .context("printer has no controllable light")?;
    Ok(object.split_once(' ').map_or(object, |(_, name)| name))
}

fn light_script(selection: &ObjectSelection, on: bool) -> anyhow::Result<String> {
    let level = u8::from(on);
    Ok(format!(
        "SET_LED LED={} RED={level} GREEN={level} BLUE={level} WHITE={level}",
        light_name(selection)?
    ))
}

async fn light_is_on(
    client: &MoonrakerClient,
    selection: &ObjectSelection,
) -> anyhow::Result<bool> {
    let object = selection
        .light
        .clone()
        .context("printer has no controllable light")?;
    let mut status = KlipperStatus::default();
    status.apply(
        client
            .query_objects(std::slice::from_ref(&object))
            .await
            .context("query light state")?,
    );
    let led: Led = status.object(&object);
    Ok(led
        .color_data
        .iter()
        .flatten()
        .any(|channel| *channel > 0.0))
}
