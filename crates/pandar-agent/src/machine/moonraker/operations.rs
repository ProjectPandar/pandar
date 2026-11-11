use anyhow::{Context, bail};
use pandar_core::{PrinterAxis, PrinterOperation};

use super::{
    MoonrakerClient,
    client::PrintAction,
    projection::speed_factor_percent,
    status::{KlipperStatus, Led, ObjectSelection},
};

const DEFAULT_MOVE_FEEDRATE_MM_PER_MIN: u32 = 3000;
const PART_COOLING_FAN_INDEX: u8 = 1;

pub(super) async fn operate(
    client: &MoonrakerClient,
    selection: &ObjectSelection,
    operation: PrinterOperation,
) -> anyhow::Result<()> {
    let action = operation.action();
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
            fan_index: PART_COOLING_FAN_INDEX,
            speed_percent,
            ..
        } => format!(
            "M106 S{}",
            (u32::from(speed_percent.min(100)) * 255 + 50) / 100
        ),
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
        PrinterOperation::SetHotendTemperature {
            temperature_celsius,
            wait,
            extruder_id,
        } => {
            let command = if wait { "M109" } else { "M104" };
            match extruder_id {
                Some(index) => format!("{command} T{index} S{temperature_celsius}"),
                None => format!("{command} S{temperature_celsius}"),
            }
        }
        PrinterOperation::SetBedTemperature {
            temperature_celsius,
            wait,
        } => format!(
            "{} S{temperature_celsius}",
            if wait { "M190" } else { "M140" }
        ),
        PrinterOperation::SetChamberTemperature {
            temperature_celsius,
            wait,
        } => {
            let heater = selection
                .chamber
                .as_deref()
                .and_then(|name| name.strip_prefix("heater_generic "))
                .context("printer has no controllable chamber heater")?;
            let mut script =
                format!("SET_HEATER_TEMPERATURE HEATER={heater} TARGET={temperature_celsius}");
            if wait {
                script.push_str(&format!(
                    "\nTEMPERATURE_WAIT SENSOR=\"heater_generic {heater}\" MINIMUM={temperature_celsius}"
                ));
            }
            script
        }
        PrinterOperation::AmsLoadFilament { slot_id, .. } if selection.filament_channels => {
            format!("AUTO_FEEDING EXTRUDER={slot_id} LOAD=1")
        }
        PrinterOperation::AmsUnloadFilament { slot_id, .. } if selection.filament_channels => {
            format!("AUTO_FEEDING EXTRUDER={slot_id} UNLOAD=1")
        }
        PrinterOperation::AmsRereadRfid { slot_id, .. } if selection.filament_channels => {
            format!("FILAMENT_DT_UPDATE CHANNEL={slot_id}")
        }
        _ => bail!("printer operation {action} is not supported on Moonraker printers"),
    };
    client
        .run_gcode(&script)
        .await
        .with_context(|| format!("run Moonraker {action} G-code"))
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
