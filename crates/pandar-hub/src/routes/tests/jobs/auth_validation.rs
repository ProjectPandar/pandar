use super::*;

#[tokio::test]
async fn linked_viewer_jwt_cannot_create_print_job() {
    let state = state().await;
    let app = router(external_auth_state(state.clone()));
    let tenant = state.tenants().create("acme", "Acme Labs").await.unwrap();
    let token = external_auth_token_for_role(
        &state,
        tenant.id,
        crate::repositories::UserRole::Viewer,
        "linked-job-viewer",
    )
    .await;
    let agent = state.agents().create(tenant.id, "agent").await.unwrap();
    let printer_id = insert_printer_fixture(state.database(), tenant.id, agent.id)
        .await
        .unwrap();

    let (status, body) = multipart_request_as(
        app,
        Method::POST,
        &format!("/api/v1/tenants/{}/printers/{printer_id}/jobs", tenant.id),
        multipart_print_body(None, Some(("plate.3mf", "model/3mf", b"abc")), 1),
        &token,
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(decode::<ErrorResponse>(body).error, "role_forbidden");
}

#[tokio::test]
async fn job_create_defaults_content_type_and_rejects_invalid_plate() {
    let state = state().await;
    let app = router(state.clone());
    let (_, tenant) = create_tenant_for_test(app.clone()).await;
    let tenant_id = TenantId::parse(&decode::<TenantResponse>(tenant).id).unwrap();
    let token = auth_token_for_role(
        &state,
        &tenant_id.to_string(),
        crate::repositories::UserRole::Operator,
        "default-content-type-operator",
    )
    .await;
    let agent = state.agents().create(tenant_id, "agent").await.unwrap();
    let printer_id = insert_printer_fixture(state.database(), tenant_id, agent.id)
        .await
        .unwrap();
    let uri = format!("/api/v1/tenants/{tenant_id}/printers/{printer_id}/jobs");

    let (status, body) = multipart_request_as(
        app.clone(),
        Method::POST,
        &uri,
        multipart_print_body_with_fields(
            Some(("plate.3mf", "", b"abc")),
            &[
                ("filename", "plate.3mf"),
                ("content_type", ""),
                ("plate_id", "1"),
                ("use_ams", "false"),
                ("bed_leveling", "false"),
                ("auto_bed_leveling", "0"),
                ("flow_cali", "false"),
                ("auto_flow_cali", "0"),
                ("auto_offset_cali", "0"),
                ("timelapse", "false"),
            ],
        ),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(
        decode::<JobResponse>(body).artifact.content_type,
        "application/octet-stream"
    );

    for plate_id in [0, -1] {
        let (status, body) = multipart_request_as(
            app.clone(),
            Method::POST,
            &uri,
            multipart_print_body(None, Some(("plate.3mf", "model/3mf", b"abc")), plate_id),
            &token,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            decode::<ErrorResponse>(body).error,
            "artifact_invalid_plate"
        );
    }

    let (status, body) = multipart_request_as(
        app,
        Method::POST,
        &uri,
        multipart_print_body(
            None,
            Some(("plate.3mf", "model/3mf", b"abc")),
            4294967296_i64,
        ),
        &token,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        decode::<ErrorResponse>(body).error,
        "artifact_invalid_plate"
    );
}
