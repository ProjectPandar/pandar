use super::*;

#[tokio::test]
async fn refresh_printers_multiple_snapshots_stay_ordered() {
    let config = test_config();
    let command_id = uuid::Uuid::new_v4().to_string();
    let gateway = FakeGateway::ok([
        snapshot("SERIAL1", "first", Some("A1 Mini"), "READY"),
        snapshot("SERIAL2", "second", None, "RUNNING"),
    ]);
    let (sender, mut receiver) = mpsc::channel(4);

    handle_command_with_gateway(
        &config,
        &gateway,
        &sender,
        refresh_command(command_id.clone()),
    )
    .await
    .unwrap();
    drop(sender);

    assert_eq!(
        receiver.recv().await.unwrap(),
        ack_event(&config, &command_id)
    );
    assert_snapshot(
        receiver.recv().await.unwrap(),
        "SERIAL1",
        "first",
        "A1 Mini",
        "READY",
    );
    assert_snapshot(
        receiver.recv().await.unwrap(),
        "SERIAL2",
        "second",
        "",
        "RUNNING",
    );
    assert_eq!(
        receiver.recv().await.unwrap(),
        success_event(&config, &command_id)
    );
    assert!(receiver.recv().await.is_none());
}

#[tokio::test]
async fn refresh_printers_gateway_failure_emits_ack_and_failed_result_with_context() {
    let config = test_config();
    let command_id = uuid::Uuid::new_v4().to_string();
    let gateway = FakeGateway::fail();
    let (sender, mut receiver) = mpsc::channel(2);

    handle_command_with_gateway(
        &config,
        &gateway,
        &sender,
        refresh_command(command_id.clone()),
    )
    .await
    .unwrap();
    drop(sender);

    assert_eq!(
        receiver.recv().await.unwrap(),
        ack_event(&config, &command_id)
    );
    let failure = receiver.recv().await.unwrap();
    assert!(receiver.recv().await.is_none());

    match failure.event.unwrap() {
        agent_event::Event::CommandResult(result) => {
            assert_eq!(result.command_id, command_id);
            assert!(!result.success);
            assert!(result.error.contains("refresh failed"));
            assert!(result.error.contains("transport unavailable"));
        }
        other => panic!("expected command result, got {other:?}"),
    }
}
