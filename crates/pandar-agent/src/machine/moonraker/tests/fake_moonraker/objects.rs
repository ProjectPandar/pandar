//! The Klipper object tree a Snapmaker U1 exposes, stored as open-ended JSON.

use serde_json::{Map, Value, json};

pub(super) fn initial_objects() -> Map<String, Value> {
    let Value::Object(objects) = json!({
        "webhooks": {"state": "ready", "state_message": "Printer is ready"},
        "print_stats": {
            "state": "standby", "filename": "", "print_duration": 0.0, "message": "",
            "info": {"current_layer": null, "total_layer": null}
        },
        "virtual_sdcard": {"progress": 0.0},
        "display_status": {"progress": 0.0},
        "toolhead": {"extruder": "extruder"},
        "gcode_move": {"speed_factor": 1.0},
        "heater_bed": {"temperature": 24.5, "target": 0.0},
        "fan": {"speed": 1.0},
        "fan_generic e1_fan": {"speed": 0.5},
        "fan_generic e2_fan": {"speed": 0.0},
        "fan_generic e3_fan": {"speed": 0.0},
        "fan_generic cavity_fan": {"speed": 0.3},
        "fan_generic exhaust_fan": {"speed": 0.6},
        "fan_generic circulation_fan": {"speed": 0.0},
        "heater_fan power_fan": {"speed": 1.0},
        "heater_fan e0_nozzle_fan": {"speed": 1.0},
        "heater_fan e1_nozzle_fan": {"speed": 0.8},
        "heater_fan e2_nozzle_fan": {"speed": 0.0},
        "heater_fan e3_nozzle_fan": {"speed": 0.0},
        "extruder": {"temperature": 25.0, "target": 0.0, "nozzle_diameter": 0.4},
        "extruder1": {"temperature": 25.5, "target": 0.0, "nozzle_diameter": 0.6},
        "extruder2": {"temperature": 26.0, "target": 0.0, "nozzle_diameter": 0.4},
        "extruder3": {"temperature": 26.5, "target": 0.0, "nozzle_diameter": 0.8},
        "led cavity_led": {"color_data": [[0.0, 0.0, 0.0, 0.0]]},
        "filament_detect": {"info": []},
        "purifier": {"power_detected": false, "exhaust_fan": {"speed": 0.6}},
        "gcode_macro AUTO_FEEDING": {"is_command": true},
        "print_task_config": {
            "filament_exist": [true, true, false, false],
            "filament_vendor": ["Snapmaker", "Generic", "", ""],
            "filament_type": ["PLA", "PETG", "", ""],
            "filament_sub_type": ["Basic", "", "", ""],
            "filament_color_rgba": ["ff0000ff", "00ff00ff", "", ""]
        }
    }) else {
        unreachable!("initial objects are a JSON object")
    };
    objects
}
