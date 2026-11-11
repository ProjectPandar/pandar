use super::*;

#[test]
fn project_file_payload_matches_studio_auto_calibration_combination() {
    let payload = BambuMqttCommand::project_file(ProjectFileCommand {
        printer_model: Some("N6".to_owned()),
        plate_id: 1,
        auto_bed_leveling: PrintCalibrationMode::Auto,
        auto_flow_cali: PrintCalibrationMode::Auto,
        timelapse: true,
        ..project_file_command()
    })
    .payload();

    let print = project_file_payload(&payload).print;
    assert!(!print.bed_leveling);
    assert_eq!(print.auto_bed_leveling, 2);
    assert!(!print.flow_cali);
    assert_eq!(print.extrude_cali_flag, 2);
    assert_eq!(print.nozzle_offset_cali, 0);
    assert!(print.timelapse);
}
#[test]
fn project_file_payload_defaults_mapping_keys_when_no_mapping_supplied() {
    let payload = BambuMqttCommand::project_file(ProjectFileCommand {
        use_ams: false,
        ..project_file_command()
    })
    .payload();

    let print = project_file_payload(&payload).print;
    assert_eq!(print.ams_mapping, Vec::<i64>::new());
    assert_eq!(print.ams_mapping2, Vec::<TestAmsMapping2>::new());
    assert!(!print.use_ams);
}
