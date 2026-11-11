use super::*;

#[tokio::test]
async fn gcode_line_operation_round_trips_exact_param_and_audit_omits_content() {
    let (database, tenants, agents, _, commands, _) = repositories().await;
    let audit = AuditEventRepository::new(database.clone());
    let tenant = tenants.create("gcode-line", "G-code Line").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id = crate::repositories::test_helpers::insert_printer_fixture_with_model(
        &database,
        tenant.id,
        agent.id,
        Some("A1"),
    )
    .await
    .unwrap();
    let param = "M620 C1 \r\n; keep  \n";
    let operation = PrinterOperationKind::GcodeLine {
        param: param.to_owned(),
    };
    assert!(operation.required_device_features().is_empty());

    let command = commands
        .enqueue_printer_operation_with_audit(
            tenant.id,
            &printer_id,
            operation.clone(),
            test_audit_actor(),
        )
        .await
        .unwrap();
    let payload: PrinterOperationPayload = serde_json::from_str(&command.payload_json).unwrap();

    assert_eq!(command.status, CommandStatus::Queued);
    assert_eq!(payload.operation, operation);
    let event = audit
        .list_for_tenant(tenant.id)
        .await
        .unwrap()
        .into_iter()
        .find(|event| event.action == "printer.dispatch_control")
        .expect("printer control audit event");
    let metadata: serde_json::Value = serde_json::from_str(&event.metadata_json).unwrap();
    assert_eq!(metadata["action"], "gcode_line");
    assert!(metadata.get("param").is_none());
    assert!(!event.metadata_json.contains("M620 C1"));
}
