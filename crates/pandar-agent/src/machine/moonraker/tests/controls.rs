//! The control sequence the Moonraker E2E drives: print lifecycle, heaters,
//! tool select, every fan index, filament commands, a rejected drying command
//! and a dwell longer than the dispatch grace.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::time::timeout;

use super::fake_moonraker::FakeMoonraker;
use super::support::{Harness, operate};
use pandar_protocol::agent::v1::{
    AmsLoadFilamentOperation, AmsRereadRfidOperation, AmsStartDryingOperation, Axis, AxisMovement,
    GcodeLineOperation, HomeOperation, MoveAxesOperation, PauseOperation, SelectExtruderOperation,
    SetBedTemperatureOperation, SetChamberLightOperation, SetFanSpeedOperation,
    SetHotendTemperatureOperation, SetPrintSpeedOperation, printer_operation,
};

/// Runs every operation in order and returns one transcript entry per command.
pub(super) async fn run_control_operations(
    harness: &mut Harness,
    printer: &FakeMoonraker,
) -> Vec<Value> {
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
            "hotend-t2",
            printer_operation::Operation::SetHotendTemperature(SetHotendTemperatureOperation {
                temperature_celsius: 220,
                wait: false,
                extruder_id: Some(2),
            }),
        ),
        (
            "select-t3",
            printer_operation::Operation::SelectExtruder(SelectExtruderOperation {
                extruder_id: 3,
            }),
        ),
        (
            "part-fan",
            printer_operation::Operation::SetFanSpeed(SetFanSpeedOperation {
                fan_index: 1,
                speed_percent: 0,
                airduct: false,
            }),
        ),
        (
            "aux-fan",
            printer_operation::Operation::SetFanSpeed(SetFanSpeedOperation {
                fan_index: 2,
                speed_percent: 50,
                airduct: false,
            }),
        ),
        (
            "chamber-fan",
            printer_operation::Operation::SetFanSpeed(SetFanSpeedOperation {
                fan_index: 3,
                speed_percent: 100,
                airduct: false,
            }),
        ),
        (
            "load",
            printer_operation::Operation::AmsLoadFilament(AmsLoadFilamentOperation {
                ams_id: 0,
                slot_id: 1,
                ..Default::default()
            }),
        ),
        (
            "reread",
            printer_operation::Operation::AmsRereadRfid(AmsRereadRfidOperation {
                ams_id: 0,
                slot_id: 0,
            }),
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
        (
            // Longer than the dispatch grace: the Agent must report it
            // dispatched and let the script finish in the background.
            "dwell",
            printer_operation::Operation::GcodeLine(GcodeLineOperation {
                param: "G4 P4000".to_owned(),
            }),
        ),
    ] {
        let started = std::time::Instant::now();
        harness.run(command_id, operate(operation)).await;
        let result = harness.result(command_id).await;
        operations.push(json!({
            "command": command_id,
            "success": result.success,
            "elapsed_seconds": if command_id == "dwell" {
                Some(started.elapsed().as_secs())
            } else {
                None
            },
        }));
    }
    // The detached request stays alive until Klipper answers.
    timeout(Duration::from_secs(15), async {
        loop {
            if printer
                .requests()
                .await
                .iter()
                .any(|r| r.starts_with("gcode-done"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    })
    .await
    .expect("the detached dwell must finish");

    operations
}
