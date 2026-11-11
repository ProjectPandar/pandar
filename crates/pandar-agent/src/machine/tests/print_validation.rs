use super::*;

#[tokio::test]
async fn configured_print_project_file_rejects_unknown_flow_cali_before_upload() {
    let mqtt = FakeMqttTransport::default();
    let transfer = FakeMachineFileTransfer::default();
    let mut endpoint = endpoint("SERIAL1");
    endpoint.model = None;
    let gateway = ConfiguredBambuMachineGateway::with_file_transfer(
        vec![(endpoint, mqtt.clone(), transfer.clone())],
        Duration::from_secs(1),
    );
    let mut command = print_project_file();
    command.options.as_mut().unwrap().flow_cali = true;

    let err = gateway
        .print_project_file("SERIAL1", &command, b"abc".to_vec())
        .await
        .unwrap_err();

    assert!(format!("{err:#}").contains("flow calibration"));
    assert!(transfer.recorded_requests().is_empty());
    assert!(mqtt.published_commands().await.is_empty());
}

#[tokio::test]
async fn configured_print_project_file_allows_disabled_calibrations_for_unknown_model() {
    let mqtt = FakeMqttTransport::default();
    let transfer = FakeMachineFileTransfer::default();
    let mut printer = endpoint("SERIAL1");
    printer.model = None;
    let gateway = ConfiguredBambuMachineGateway::with_file_transfer(
        vec![(printer, mqtt.clone(), transfer.clone())],
        Duration::from_secs(1),
    );

    gateway
        .print_project_file("SERIAL1", &print_project_file(), b"abc".to_vec())
        .await
        .unwrap();

    assert!(!transfer.recorded_requests().is_empty());
    assert!(!mqtt.published_commands().await.is_empty());
}

#[tokio::test]
async fn configured_print_project_file_rejects_missing_options_before_upload() {
    let mqtt = FakeMqttTransport::default();
    let transfer = FakeMachineFileTransfer::default();
    let gateway = ConfiguredBambuMachineGateway::with_file_transfer(
        vec![(endpoint("SERIAL1"), mqtt.clone(), transfer.clone())],
        Duration::from_secs(1),
    );
    let mut command = print_project_file();
    command.options = None;

    let err = gateway
        .print_project_file("SERIAL1", &command, b"abc".to_vec())
        .await
        .unwrap_err();

    assert!(format!("{err:#}").contains("missing print project file options"));
    assert!(transfer.recorded_requests().is_empty());
    assert!(mqtt.published_commands().await.is_empty());
}
