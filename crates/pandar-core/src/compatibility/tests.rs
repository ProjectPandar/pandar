use super::*;

#[test]
fn normalizes_aliases_and_friendly_model_names() {
    for (model, expected) in [
        ("N1", "A1_MINI"),
        ("N2S", "A1"),
        ("N7", "P2S"),
        ("N6", "X2D"),
        ("BL-P001", "X1C"),
        ("BL-P002", "X1"),
        ("C13", "X1E"),
        ("C11", "P1P"),
        ("C12", "P1S"),
        ("N9", "A2L"),
        ("O1C", "H2C"),
        ("O1C2", "H2C"),
        ("O1D", "H2D"),
        ("O1E", "H2D_PRO"),
        ("O1S", "H2S"),
        ("Bambu Lab H2D", "H2D"),
        ("Bambu Lab H2C", "H2C"),
        ("Bambu Lab H2S", "H2S"),
        ("Bambu Lab A2L", "A2L"),
        ("Bambu Lab P1P", "P1P"),
        ("Bambu Lab P1S", "P1S"),
        ("X1 Carbon", "X1C"),
        ("3DPrinter-X1-Carbon", "X1C"),
        ("Bambu Lab X1E", "X1E"),
        ("Bambu Lab P2S", "P2S"),
        ("Bambu Lab X2D", "X2D"),
        ("A1 Mini", "A1_MINI"),
        ("bambu lab a1 mini", "A1_MINI"),
        (" a1-mini ", "A1_MINI"),
    ] {
        assert_eq!(normalize_model(model).as_deref(), Some(expected), "{model}");
    }
    assert_eq!(normalize_model(" ").as_deref(), None);
}

#[test]
fn print_option_projection_covers_every_verified_model_family() {
    let on_off = || CalibrationOption {
        modes: vec![PrintCalibrationMode::On, PrintCalibrationMode::Off],
        default_mode: PrintCalibrationMode::On,
    };
    let auto = |default_mode| CalibrationOption {
        modes: vec![
            PrintCalibrationMode::Auto,
            PrintCalibrationMode::On,
            PrintCalibrationMode::Off,
        ],
        default_mode,
    };
    let cases = [
        (
            &["N6", "X2D", "Bambu Lab X2D"][..],
            PrintOptionCapabilities {
                timelapse: true,
                bed_leveling: Some(auto(PrintCalibrationMode::Auto)),
                flow_calibration: Some(auto(PrintCalibrationMode::Auto)),
                nozzle_offset_calibration: Some(auto(PrintCalibrationMode::Off)),
            },
        ),
        (
            &["N7", "P2S", "O1S", "H2S", "N9", "A2L"][..],
            PrintOptionCapabilities {
                timelapse: true,
                bed_leveling: Some(auto(PrintCalibrationMode::Auto)),
                flow_calibration: Some(auto(PrintCalibrationMode::Auto)),
                nozzle_offset_calibration: None,
            },
        ),
        (
            &[
                "N1", "A1 Mini", "N2S", "A1", "BL-P001", "X1C", "BL-P002", "X1", "C13", "X1E",
            ][..],
            PrintOptionCapabilities {
                timelapse: true,
                bed_leveling: Some(on_off()),
                flow_calibration: Some(on_off()),
                nozzle_offset_calibration: None,
            },
        ),
        (
            &["C11", "P1P", "C12", "P1S"][..],
            PrintOptionCapabilities {
                timelapse: true,
                bed_leveling: Some(on_off()),
                flow_calibration: None,
                nozzle_offset_calibration: None,
            },
        ),
        (
            &["O1C", "O1C2", "H2C", "O1D", "H2D", "O1E", "H2D Pro"][..],
            PrintOptionCapabilities {
                timelapse: true,
                bed_leveling: Some(auto(PrintCalibrationMode::Auto)),
                flow_calibration: Some(auto(PrintCalibrationMode::Auto)),
                nozzle_offset_calibration: Some(auto(PrintCalibrationMode::Auto)),
            },
        ),
    ];

    for (models, expected) in cases {
        for model in models {
            assert_eq!(
                compatibility_for_model(Some(model)).print_options,
                expected,
                "{model}"
            );
        }
    }
}
