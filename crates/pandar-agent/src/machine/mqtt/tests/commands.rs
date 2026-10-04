use super::*;

#[test]
fn studio_command_payloads_use_incrementing_studio_sequence_ids() {
    let commands = [
        (BambuMqttCommand::GetVersion.payload(), "info"),
        (BambuMqttCommand::RequestPushAll.payload(), "pushing"),
        (BambuMqttCommand::PausePrint.payload(), "print"),
        (BambuMqttCommand::ResumePrint.payload(), "print"),
        (BambuMqttCommand::StopPrint.payload(), "print"),
        (BambuMqttCommand::SetChamberLight(true).payload(), "system"),
        (
            BambuMqttCommand::SetPrintSpeed(PrintSpeed::new(4).unwrap()).payload(),
            "print",
        ),
        (
            BambuMqttCommand::GcodeLine(GcodeLineCommand {
                param: "G28".to_string(),
            })
            .payload(),
            "print",
        ),
        (
            BambuMqttCommand::project_file(ProjectFileCommand {
                flow_cali: true,
                ..project_file_command()
            })
            .payload(),
            "print",
        ),
        (
            BambuMqttCommand::AmsRereadRfid(AmsSlotCommand {
                ams_id: 0,
                slot_id: 1,
            })
            .payload(),
            "print",
        ),
        (
            BambuMqttCommand::AmsLoadFilament(AmsFilamentCommand {
                ams_id: 0,
                slot_id: 1,
                target: 1,
                extruder_id: Some(0),
            })
            .payload(),
            "print",
        ),
        (
            BambuMqttCommand::AmsUnloadFilament(AmsFilamentCommand {
                ams_id: 0,
                slot_id: 1,
                target: 1,
                extruder_id: None,
            })
            .payload(),
            "print",
        ),
        (
            BambuMqttCommand::AmsStartDrying(AmsDryingCommand {
                ams_id: 0,
                temperature_celsius: 55,
                duration_hours: 8,
                filament: "PLA".to_string(),
                rotate_tray: false,
            })
            .payload(),
            "print",
        ),
        (BambuMqttCommand::AmsStopDrying(0).payload(), "print"),
    ];

    let command_count = commands.len();
    let mut sequence_ids = Vec::new();
    for (payload, section) in commands {
        sequence_ids.push(studio_sequence_id(&payload, section));
    }
    sequence_ids.sort();
    sequence_ids.dedup();
    assert_eq!(sequence_ids.len(), command_count);
}

#[test]
fn studio_sequence_id_wraps_before_leaving_studio_range() {
    let sequence = AtomicU32::new(29999);

    let last = next_studio_sequence_id_from(&sequence);
    let wrapped = next_studio_sequence_id_from(&sequence);
    let continued = next_studio_sequence_id_from(&sequence);

    assert_eq!(last, "29999");
    assert_eq!(wrapped, "20000");
    assert_eq!(continued, "20001");

    let out_of_range = AtomicU32::new(30000);
    assert_eq!(next_studio_sequence_id_from(&out_of_range), "20000");
    assert_eq!(next_studio_sequence_id_from(&out_of_range), "20001");
}

#[test]
fn ams_drying_payloads_match_bambu_studio_reference() {
    let start = BambuMqttCommand::AmsStartDrying(AmsDryingCommand {
        ams_id: 0,
        temperature_celsius: 55,
        duration_hours: 8,
        filament: "PETG".to_string(),
        rotate_tray: true,
    })
    .payload();
    assert_eq!(
        start,
        serde_json::json!({
            "print": {
                "command": "ams_filament_drying",
                "sequence_id": studio_sequence_id(&start, "print"),
                "ams_id": 0,
                "mode": 1,
                "filament": "PETG",
                "temp": 55,
                "duration": 8,
                "humidity": 0,
                "rotate_tray": true,
                "cooling_temp": 20,
                "close_power_conflict": false,
            }
        })
    );

    let stop = BambuMqttCommand::AmsStopDrying(128).payload();
    assert_eq!(
        stop,
        serde_json::json!({
            "print": {
                "command": "ams_filament_drying",
                "sequence_id": studio_sequence_id(&stop, "print"),
                "ams_id": 128,
                "mode": 0,
                "filament": "",
                "temp": 0,
                "duration": 0,
                "humidity": 0,
                "rotate_tray": false,
                "cooling_temp": 0,
                "close_power_conflict": false,
            }
        })
    );
}

#[test]
fn constants_match_bambu_defaults() {
    assert_eq!(BAMBU_MQTT_PORT, 8883);
    assert_eq!(BAMBU_MQTT_USERNAME, "bblp");
    assert_eq!(BAMBU_MQTT_QOS, 1);
}

#[test]
fn lan_mqtt_accepts_full_pushall_reports() {
    let options = bambu_lan_mqtt_options(&endpoint(), None);

    assert!(options.max_packet_size() >= 256 * 1024);
}

#[test]
fn mqtt_report_error_log_preserves_error_chain() {
    let err = anyhow!("payload size limit exceeded: 262600")
        .context("MQTT serialization/deserialization error")
        .context("poll rumqttc event loop");

    let (logs, ()) = crate::test_tracing::capture_logs(|| warn_mqtt_report_receive_failed(&err));

    let captured = logs.contents();
    assert!(captured.contains("MQTT report receive failed"));
    assert!(captured.contains("payload size limit exceeded: 262600"));
    assert!(captured.contains("poll rumqttc event loop"));
}

#[test]
fn get_version_report_extracts_trimmed_ota_model() {
    let observation = MachineReport::decode(get_version_report(" P2S "))
        .firmware_version_observation()
        .unwrap()
        .unwrap();
    assert_eq!(observation.model, "P2S");
}

#[test]
fn get_version_report_rejects_missing_model() {
    let report = get_version_report_with_blank_model();

    assert!(
        MachineReport::decode(report)
            .firmware_version_observation()
            .is_err()
    );
}

#[test]
fn basic_print_control_payloads_match_reference() {
    let pause = BambuMqttCommand::PausePrint.payload();
    let resume = BambuMqttCommand::ResumePrint.payload();
    let stop = BambuMqttCommand::StopPrint.payload();
    assert_eq!(
        pause,
        expected_print_command_payload("pause", "", &studio_sequence_id(&pause, "print"))
    );
    assert_eq!(
        resume,
        expected_print_command_payload("resume", "", &studio_sequence_id(&resume, "print"))
    );
    assert_eq!(
        stop,
        expected_print_command_payload("stop", "", &studio_sequence_id(&stop, "print"))
    );
}

#[test]
fn print_speed_is_limited_to_reference_modes() {
    let payload = BambuMqttCommand::SetPrintSpeed(PrintSpeed::new(4).unwrap()).payload();
    assert_eq!(
        payload,
        expected_print_command_payload("print_speed", "4", &studio_sequence_id(&payload, "print"))
    );
    assert!(PrintSpeed::new(0).is_err());
    assert!(PrintSpeed::new(5).is_err());
}

#[test]
fn gcode_line_payload_preserves_single_home_line() {
    let payload = BambuMqttCommand::GcodeLine(GcodeLineCommand {
        param: "G28".to_string(),
    })
    .payload();
    assert_eq!(
        payload,
        expected_print_command_payload("gcode_line", "G28", &studio_sequence_id(&payload, "print"))
    );
}

#[test]
fn gcode_line_payload_preserves_hotend_temperature_line() {
    let payload = BambuMqttCommand::GcodeLine(GcodeLineCommand {
        param: "M104 S200".to_string(),
    })
    .payload();
    assert_eq!(
        payload,
        expected_print_command_payload(
            "gcode_line",
            "M104 S200",
            &studio_sequence_id(&payload, "print")
        )
    );
}

#[test]
fn raw_json_payload_is_preserved() {
    let payload = raw_print_payload("custom", "9");
    assert_eq!(
        BambuMqttCommand::RawJson(payload.clone()).payload(),
        payload
    );
}
