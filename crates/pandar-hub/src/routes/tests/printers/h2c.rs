use super::*;

#[tokio::test]
async fn printer_list_projects_h2c_nozzle_system_only_for_current_capable_session() {
    let state = state().await;
    let app = router(state.clone());
    let (tenant, agent, token) = tenant_and_agent(&state, app.clone()).await;
    let tenant_id = TenantId::parse(&decode::<TenantResponse>(tenant).id).unwrap();
    let agent_id = AgentId::parse(&decode::<AgentResponse>(agent).id).unwrap();
    let serial = format!("h2c-{agent_id}");
    let session = register_h2c_session(&state, tenant_id, agent_id).await;
    upsert_rack_snapshot(
        &state,
        tenant_id,
        agent_id,
        &session,
        &serial,
        "O1C2",
        h2c_nozzle_system(),
    )
    .await;

    let (status, body) = request_as(
        app.clone(),
        Method::GET,
        &format!("/api/v1/tenants/{tenant_id}/printers"),
        None,
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["printers"][0]["nozzle_system"]["nozzle"]["info"][0]["id"], 16,
        "{body}"
    );

    register_h2c_session(&state, tenant_id, agent_id).await;

    let (status, body) = request_as(
        app,
        Method::GET,
        &format!("/api/v1/tenants/{tenant_id}/printers"),
        None,
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body["printers"][0].get("nozzle_system").is_none()
            || body["printers"][0]["nozzle_system"].is_null(),
        "{body}"
    );
}

#[tokio::test]
async fn printer_list_projects_h3c_rack_with_mounted_hotend_address() {
    let state = state().await;
    let app = router(state.clone());
    let (tenant, agent, token) = tenant_and_agent(&state, app.clone()).await;
    let tenant_id = TenantId::parse(&decode::<TenantResponse>(tenant).id).unwrap();
    let agent_id = AgentId::parse(&decode::<AgentResponse>(agent).id).unwrap();
    let session = register_h2c_session(&state, tenant_id, agent_id).await;
    upsert_rack_snapshot(
        &state,
        tenant_id,
        agent_id,
        &session,
        &format!("h3c-{agent_id}"),
        "N10",
        serde_json::json!({"nozzle": {"info": [
            {"id": 0, "addr_id": 6, "diameter": 0.4, "type": "HS01", "stat": 168},
            {"id": 24, "addr_id": 8, "diameter": 0.4, "type": "HS01", "stat": 160}
        ]}}),
    )
    .await;

    let (status, body) = request_as(
        app,
        Method::GET,
        &format!("/api/v1/tenants/{tenant_id}/printers"),
        None,
        &token,
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let printer = &body["printers"][0];
    assert_eq!(
        printer["compatibility"]["normalized_model"], "H3C",
        "{body}"
    );
    assert_eq!(
        printer["compatibility"]["nozzle_layout"], "single",
        "{body}"
    );
    assert_eq!(
        printer["nozzle_system"]["nozzle"]["info"],
        serde_json::json!([
            {"id": 0, "addr_id": 6, "diameter": 0.4, "type": "HS01", "stat": 168},
            {"id": 24, "addr_id": 8, "diameter": 0.4, "type": "HS01", "stat": 160}
        ]),
        "{body}"
    );
}

async fn register_h2c_session(
    state: &AppState,
    tenant_id: TenantId,
    agent_id: AgentId,
) -> crate::sessions::SessionToken {
    let token = crate::sessions::SessionToken::new();
    state
        .agents()
        .claim_online_session(
            tenant_id,
            agent_id,
            &token.persisted_id(),
            "test",
            "2026-08-01T00:00:00Z",
        )
        .await
        .unwrap();
    state
        .sessions()
        .register(crate::sessions::AgentSession {
            token,
            tenant_id,
            agent_id,
            name: "h2c-agent".to_owned(),
            version: "test".to_owned(),
            connected_at: "2026-08-01T00:00:00Z".to_owned(),
            last_heartbeat_at: "2026-08-01T00:00:00Z".to_owned(),
            wake_sender: mpsc::channel(1).0,
            close_sender: mpsc::channel(1).0,
            command_sender: mpsc::channel(1).0,
            capabilities: [pandar_protocol::agent::v1::AgentCapability::H2cAutoNozzleMapping]
                .into_iter()
                .collect(),
            pending_live_commands: crate::sessions::empty_pending_live_commands(),
            live_command_transition: Arc::new(tokio::sync::Mutex::new(())),
        })
        .await;
    token
}

fn h2c_nozzle_system() -> serde_json::Value {
    serde_json::json!({
        "nozzle": {
            "exist": 65536,
            "state": 0,
            "src_id": 16,
            "tar_id": 17,
            "info": [{"id": 16, "diameter": 0.4, "type": "XS01", "stat": 0}]
        },
        "holder": {"stat": 0, "pos": 2, "info": 0}
    })
}

async fn upsert_rack_snapshot(
    state: &AppState,
    tenant_id: TenantId,
    agent_id: AgentId,
    session: &crate::sessions::SessionToken,
    serial: &str,
    model: &str,
    nozzle_system: serde_json::Value,
) {
    let nozzle_system = serde_json::from_value(nozzle_system).unwrap();
    state
        .printers()
        .upsert_snapshot_with_device_features_if_current(
            tenant_id,
            agent_id,
            &session.persisted_id(),
            crate::repositories::PrinterSnapshotUpsert {
                printer_family: pandar_core::PrinterFamily::BambuLab,
                serial_number: serial.to_owned(),
                host: None,
                access_code: None,
                name: "Rack".to_owned(),
                model: Some(model.to_owned()),
                status: Some("idle".to_owned()),
                observed_at: "2026-08-01T00:00:00Z".to_owned(),
                nozzle_temperatures: Vec::new(),
                active_nozzle: None,
                bed_temperature_celsius: None,
                bed_target_temperature_celsius: None,
                chamber_temperature_celsius: None,
                chamber_target_temperature_celsius: None,
                chamber_light_on: None,
                cooling_system: None,
                nozzle_system: Some(nozzle_system),
                connection_authoritative: false,
                telemetry_authoritative: false,
            },
            None,
        )
        .await
        .unwrap();
}
