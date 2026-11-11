use super::*;

#[tokio::test]
async fn required_device_features_repository_rejects_invalid_semantics() {
    let (database, tenants, agents, _, commands, _) = repositories().await;
    let tenant = tenants
        .create("invalid-required-features", "Invalid Required Features")
        .await
        .unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id = crate::repositories::test_helpers::insert_printer_fixture_with_model(
        &database,
        tenant.id,
        agent.id,
        Some("A1"),
    )
    .await
    .unwrap();
    let cases = [
        serde_json::json!({
            "type": "home",
            "axes": ["x"],
            "required_device_features": ["bambu_mqtt_homing"]
        }),
        serde_json::json!({
            "type": "home",
            "axes": [],
            "required_device_features": ["bambu_mqtt_homing", "bambu_mqtt_homing"]
        }),
        serde_json::json!({
            "type": "home",
            "axes": [],
            "required_device_features": ["bambu_mqtt_axis_control"]
        }),
        serde_json::json!({
            "type": "move_axes",
            "movements": [
                {"axis": "x", "delta_mm": 1.0},
                {"axis": "y", "delta_mm": 1.0}
            ],
            "feedrate_mm_per_min": null,
            "required_device_features": ["bambu_mqtt_axis_control"]
        }),
        serde_json::json!({
            "type": "move_axes",
            "movements": [{"axis": "x", "delta_mm": 2.0}],
            "feedrate_mm_per_min": null,
            "required_device_features": ["bambu_mqtt_axis_control"]
        }),
        serde_json::json!({
            "type": "move_axes",
            "movements": [{"axis": "x", "delta_mm": 10.0}],
            "feedrate_mm_per_min": 6_000,
            "required_device_features": ["bambu_mqtt_axis_control"]
        }),
    ];

    for value in cases {
        let operation: PrinterOperationKind = serde_json::from_value(value).unwrap();
        let err = commands
            .enqueue_printer_operation_with_audit(
                tenant.id,
                &printer_id,
                operation,
                test_audit_actor(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, RepositoryError::InvalidPrinterControl));
    }
    for feature in ["unspecified", "unknown"] {
        assert!(
            serde_json::from_value::<PrinterOperationKind>(serde_json::json!({
                "type": "home",
                "axes": [],
                "required_device_features": [feature]
            }))
            .is_err()
        );
    }
    assert_eq!(commands.count().await.unwrap(), 0);
}

#[tokio::test]
async fn required_device_features_cannot_bypass_queued_dispatch_through_sent_helper() {
    let (database, tenants, agents, _, commands, _) = repositories().await;
    let audit = AuditEventRepository::new(database.clone());
    let tenant = tenants
        .create("required-features-sent", "Required Features Sent")
        .await
        .unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id = crate::repositories::test_helpers::insert_printer_fixture_with_model(
        &database,
        tenant.id,
        agent.id,
        Some("A1"),
    )
    .await
    .unwrap();
    let operation: PrinterOperationKind = serde_json::from_value(serde_json::json!({
        "type": "home",
        "axes": [],
        "required_device_features": ["bambu_mqtt_homing"]
    }))
    .unwrap();

    let err = commands
        .create_printer_operation_sent_with_audit(
            tenant.id,
            &printer_id,
            agent.id,
            operation,
            test_audit_actor(),
        )
        .await
        .unwrap_err();

    assert!(matches!(err, RepositoryError::InvalidPrinterControl));
    assert_eq!(commands.count().await.unwrap(), 0);
    assert!(audit.list_for_tenant(tenant.id).await.unwrap().is_empty());
}
