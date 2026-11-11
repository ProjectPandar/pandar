use super::support::*;

pub(crate) fn gcode_parser_maps_relative_move_to_semantic_json() {
    let result = operation_json(b"G91\nG0 X10.5 Z-0.25 F3000");

    assert_eq!(result.status, 0);
    assert_eq!(result.http_code, 200);
    assert_operation_body_eq(
        result,
        TestOperation::MoveAxes {
            movements: vec![
                TestAxisMovement {
                    axis: "x".to_owned(),
                    delta_mm: 10.5,
                },
                TestAxisMovement {
                    axis: "z".to_owned(),
                    delta_mm: -0.25,
                },
            ],
            feedrate_mm_per_min: 3000,
        },
    );
}

pub(crate) fn gcode_parser_maps_targeted_hotend_temperature_to_semantic_json() {
    let result = operation_json(b"M104 S210 T1");

    assert_eq!(result.status, 0);
    assert_eq!(result.http_code, 200);
    assert_operation_body_eq(
        result,
        TestOperation::SetHotendTemperature {
            temperature_celsius: 210,
            wait: false,
            extruder_id: Some(1),
        },
    );
}

pub(crate) fn gcode_parser_rejects_unsupported_or_ambiguous_commands() {
    for message in [
        b"G0 X10".as_slice(),
        b"G90\nG0 X10",
        b"G91 X10\nG0 Y5",
        b"G91 F3000\nG0 X5",
        b"G91\nG0 X1 E2",
        b"G91\nG0 X1\nG90",
        b"G91\nG0 X1\nM104 S200",
        b"M106 P1 S127",
    ] {
        let result = operation_json(message);

        assert_ne!(result.status, 0);
        assert_eq!(result.http_code, 400);
        assert_eq!(body(result), r#"{"error":"unsupported_printer_operation"}"#);
    }
}
