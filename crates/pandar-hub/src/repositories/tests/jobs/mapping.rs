use super::*;

#[tokio::test]
async fn terminal_usage_ignores_normal_ams_mapping2_slot_outside_tray_range() {
    let (database, tenants, agents, _, _, jobs) = repositories().await;
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();
    let created = jobs
        .create_print_job(create_input(tenant.id, agent.id, &printer_id, "artifact-1"))
        .await
        .unwrap();
    let Database::Sqlite(pool) = &database else {
        panic!("expected SQLite database");
    };
    sqlx::query("UPDATE jobs SET ams_mapping2_json = ?2 WHERE id = ?1")
        .bind(created.job.id.to_string())
        .bind(r#"[{"ams_id":0,"slot_id":8}]"#)
        .execute(pool)
        .await
        .unwrap();

    let applied = jobs
        .apply_print_report(ApplyPrintReport {
            printer_materials_json: material_patch_json("2026-06-22T00:00:00Z"),
            ..report_input(
                tenant.id,
                agent.id,
                &printer_id,
                Some(created.job.id),
                None,
                "FINISH",
            )
        })
        .await
        .unwrap();

    assert_eq!(applied.job.unwrap().job.filament_usage, Vec::new());
}

#[tokio::test]
async fn terminal_report_derives_usage_from_mapping_and_material_snapshot_idempotently() {
    let (database, tenants, agents, _, _, jobs) = repositories().await;
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();
    let mut input = create_input(tenant.id, agent.id, &printer_id, "artifact-1");
    input.options.ams_mapping_json = Some("[0,254,255,128,0]".to_string());
    let created = jobs.create_print_job(input).await.unwrap();

    let terminal = ApplyPrintReport {
        printer_materials_json: material_patch_json("2026-06-22T00:00:00Z"),
        ..report_input(
            tenant.id,
            agent.id,
            &printer_id,
            Some(created.job.id),
            None,
            "FINISH",
        )
    };
    let applied = jobs.apply_print_report(terminal.clone()).await.unwrap();
    let replay = jobs.apply_print_report(terminal).await.unwrap();

    let usage = applied.job.unwrap().job.filament_usage;
    assert_eq!(usage.len(), 4);
    assert_eq!(usage[0].slot_index, 0);
    assert_eq!(usage[0].source, "ams_mapping");
    assert_eq!(usage[0].ams_id.as_deref(), Some("0"));
    assert_eq!(usage[0].tray_id.as_deref(), Some("0"));
    assert_eq!(usage[0].global_tray_id, Some(0));
    assert_eq!(usage[0].filament_type.as_deref(), Some("PLA"));
    assert_eq!(usage[1].external_id.as_deref(), Some("254"));
    assert_eq!(usage[1].tray_id.as_deref(), Some("0"));
    assert_eq!(usage[1].filament_type.as_deref(), Some("PETG"));
    assert_eq!(usage[2].ams_id.as_deref(), Some("128"));
    assert_eq!(usage[2].global_tray_id, None);
    assert_eq!(usage[3].slot_index, 4);
    assert_eq!(usage[3].filament_type.as_deref(), Some("PLA"));
    assert_eq!(replay.job.unwrap().job.filament_usage.len(), 4);
    assert_eq!(
        jobs.filament_usage_count(tenant.id, created.job.id)
            .await
            .unwrap(),
        4
    );
}

#[tokio::test]
async fn mapping2_takes_precedence_and_external_slots_match_materials() {
    let (database, tenants, agents, _, _, jobs) = repositories().await;
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();
    let mut input = create_input(tenant.id, agent.id, &printer_id, "artifact-1");
    input.options.ams_mapping_json = Some("[0,0,0]".to_string());
    input.options.ams_mapping2_json = Some(
        r#"[{"ams_id":254,"slot_id":8},{"ams_id":255,"slot_id":255},{"ams_id":2,"slot_id":3}]"#
            .to_string(),
    );
    let created = jobs.create_print_job(input).await.unwrap();

    let applied = jobs
        .apply_print_report(ApplyPrintReport {
            printer_materials_json: material_patch_json("2026-06-22T00:00:00Z"),
            ..report_input(
                tenant.id,
                agent.id,
                &printer_id,
                Some(created.job.id),
                None,
                "FAILED",
            )
        })
        .await
        .unwrap();

    let usage = applied.job.unwrap().job.filament_usage;
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0].slot_index, 0);
    assert_eq!(usage[0].source, "ams_mapping2");
    assert_eq!(usage[0].external_id.as_deref(), Some("254"));
    assert_eq!(usage[0].tray_id.as_deref(), Some("8"));
    assert_eq!(usage[0].filament_type.as_deref(), Some("TPU"));
    assert_eq!(usage[1].slot_index, 2);
    assert_eq!(usage[1].source, "ams_mapping2");
    assert_eq!(usage[1].global_tray_id, Some(11));
}

#[tokio::test]
async fn terminal_usage_preserves_a2l_mixed_ams_lite_global_routes() {
    let (database, tenants, agents, _, _, jobs) = repositories().await;
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();
    let mut input = create_input(tenant.id, agent.id, &printer_id, "artifact-1");
    input.options.ams_mapping_json = Some("[24,25]".to_owned());
    input.options.ams_mapping2_json = Some(r#"[{"ams_id":0,"slot_id":0}]"#.to_owned());
    let created = jobs.create_print_job(input).await.unwrap();

    let applied = jobs
        .apply_print_report(ApplyPrintReport {
            printer_materials_json: a2l_material_patch_json("2026-06-22T00:00:00Z"),
            ..report_input(
                tenant.id,
                agent.id,
                &printer_id,
                Some(created.job.id),
                None,
                "FINISH",
            )
        })
        .await
        .unwrap();

    let usage = applied.job.unwrap().job.filament_usage;
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0].source, "ams_mapping2");
    assert_eq!(usage[0].ams_id.as_deref(), Some("0"));
    assert_eq!(usage[0].tray_id.as_deref(), Some("0"));
    assert_eq!(usage[0].global_tray_id, Some(24));
    assert_eq!(usage[0].filament_type.as_deref(), Some("PLA"));
    assert_eq!(usage[1].source, "ams_mapping");
    assert_eq!(usage[1].global_tray_id, Some(25));
    assert_eq!(usage[1].filament_type.as_deref(), Some("PETG"));
}

#[tokio::test]
async fn terminal_usage_preserves_flat_a2l_route_over_stale_material_snapshot() {
    let (database, tenants, agents, _, _, jobs) = repositories().await;
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();
    let mut input = create_input(tenant.id, agent.id, &printer_id, "artifact-1");
    input.options.ams_mapping_json = Some("[24]".to_owned());
    input.options.ams_mapping2_json = Some(r#"[{"ams_id":0,"slot_id":0}]"#.to_owned());
    let created = jobs.create_print_job(input).await.unwrap();

    let applied = jobs
        .apply_print_report(ApplyPrintReport {
            printer_materials_json: material_patch_json("2026-06-22T00:00:00Z"),
            ..report_input(
                tenant.id,
                agent.id,
                &printer_id,
                Some(created.job.id),
                None,
                "FINISH",
            )
        })
        .await
        .unwrap();

    let usage = applied.job.unwrap().job.filament_usage;
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].source, "ams_mapping2");
    assert_eq!(usage[0].ams_id.as_deref(), Some("0"));
    assert_eq!(usage[0].tray_id.as_deref(), Some("0"));
    assert_eq!(usage[0].global_tray_id, Some(24));
    assert_eq!(usage[0].filament_type.as_deref(), Some("PLA"));
}

#[tokio::test]
async fn running_report_updates_material_snapshot_without_deriving_usage() {
    let (database, tenants, agents, _, _, jobs) = repositories().await;
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();
    let mut input = create_input(tenant.id, agent.id, &printer_id, "artifact-1");
    input.options.ams_mapping_json = Some("[0]".to_string());
    let created = jobs.create_print_job(input).await.unwrap();

    let applied = jobs
        .apply_print_report(ApplyPrintReport {
            printer_materials_json: material_patch_json("2026-06-22T00:00:00Z"),
            ..report_input(
                tenant.id,
                agent.id,
                &printer_id,
                Some(created.job.id),
                None,
                "RUNNING",
            )
        })
        .await
        .unwrap();

    assert_eq!(applied.job.unwrap().job.filament_usage, Vec::new());
    assert_eq!(
        jobs.filament_usage_count(tenant.id, created.job.id)
            .await
            .unwrap(),
        0
    );
    assert_eq!(material_snapshot_count(&database).await, 1);
}
