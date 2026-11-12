//! H3C filament load/unload, as Studio's rack filament page sends it
//! (PageFilament.cpp). The run writes
//! `target/e2e-artifacts/h3c-filament-commands.json`, every request the
//! printer received.

use serde_json::json;

use super::*;

#[tokio::test]
async fn h3c_filament_commands_address_each_hotend_like_studio() {
    let mqtt = FakeMqttTransport::default();
    let mut h3c = endpoint("SERIAL1");
    h3c.model = Some("N10".to_owned());
    let gateway = ConfiguredBambuMachineGateway::with_file_transfer(
        vec![(h3c, mqtt.clone(), FakeMachineFileTransfer::default())],
        Duration::from_secs(1),
    );

    gateway
        .operate_printer(
            "SERIAL1",
            PrinterOperation::AmsLoadFilament {
                ams_id: 32,
                slot_id: 2,
                global_tray_id: Some(30),
                external_id: None,
                extruder_id: None,
            },
        )
        .await
        .unwrap();
    gateway
        .operate_printer(
            "SERIAL1",
            PrinterOperation::AmsUnloadFilament {
                ams_id: 32,
                slot_id: 2,
                global_tray_id: Some(30),
                external_id: None,
                extruder_id: None,
            },
        )
        .await
        .unwrap();
    gateway
        .operate_printer(
            "SERIAL1",
            PrinterOperation::AmsUnloadFilament {
                ams_id: 255,
                slot_id: 0,
                global_tray_id: Some(250),
                external_id: Some("250".to_owned()),
                extruder_id: None,
            },
        )
        .await
        .unwrap();
    // Studio's own external-input unload, as the plugin forwards it.
    gateway
        .operate_printer(
            "SERIAL1",
            PrinterOperation::AmsUnloadFilament {
                ams_id: 249,
                slot_id: 255,
                global_tray_id: None,
                external_id: None,
                extruder_id: Some(0),
            },
        )
        .await
        .unwrap();
    let external_load = gateway
        .operate_printer(
            "SERIAL1",
            PrinterOperation::AmsLoadFilament {
                ams_id: 255,
                slot_id: 0,
                global_tray_id: Some(250),
                external_id: Some("250".to_owned()),
                extruder_id: None,
            },
        )
        .await
        .unwrap_err();

    let published = mqtt.published_commands().await;
    write_artifact(&json!({
        "requests": published.iter().map(|command| &command.payload).collect::<Vec<_>>(),
        "external_input_load_error": format!("{external_load:#}"),
    }));
    assert!(
        published
            .iter()
            .all(|command| command.topic == "device/SERIAL1/request")
    );
    let payloads = published
        .into_iter()
        .map(|command| command.payload)
        .collect::<Vec<_>>();
    let sequence = |index: usize| dynamic_sequence_id(&payloads[index]);
    assert_eq!(
        payloads,
        vec![
            // Load names the AMS 3H unit as its own target.
            expected_ams_change_filament_payload(32, 2, 32, Some(0), -1, -1, &sequence(0)),
            // Unload names slot k's feeder as 255 - k.
            expected_ams_change_filament_payload(32, 253, 255, Some(0), 210, 210, &sequence(1)),
            // An external input is addressed by its own id, not the main hotend's 255.
            expected_ams_change_filament_payload(250, 255, 255, Some(0), 210, 210, &sequence(2)),
            // Studio's already-encoded unload passes through unchanged.
            expected_ams_change_filament_payload(249, 255, 255, Some(0), 210, 210, &sequence(3)),
        ]
    );
    assert!(
        format!("{external_load:#}").contains("external input 250 cannot feed filament"),
        "{external_load:#}"
    );
}

fn write_artifact(transcript: &Value) {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/e2e-artifacts");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("h3c-filament-commands.json"),
        serde_json::to_vec_pretty(transcript).unwrap(),
    )
    .unwrap();
}
