use super::*;

#[test]
fn print_calibration_mode_uses_studio_wire_values() {
    for (mode, value) in [
        (PrintCalibrationMode::Off, 0),
        (PrintCalibrationMode::On, 1),
        (PrintCalibrationMode::Auto, 2),
    ] {
        assert_eq!(mode.as_u8(), value);
        assert_eq!(
            serde_json::to_value(mode).unwrap(),
            serde_json::json!(value)
        );
        assert_eq!(
            serde_json::from_value::<PrintCalibrationMode>(serde_json::json!(value)).unwrap(),
            mode
        );
    }
    assert_eq!(
        PrintCalibrationMode::try_from(3),
        Err(CoreError::InvalidPrintCalibrationMode(3))
    );
}
