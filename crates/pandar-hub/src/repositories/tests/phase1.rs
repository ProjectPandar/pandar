use serde::Deserialize;

use super::*;
use crate::{
    db::{Database, DatabaseConfig},
    repositories::test_helpers::{insert_command_fixture, insert_printer_fixture},
};

mod persistence;

#[test]
fn phase_13_command_result_migrations_are_backend_equivalent() {
    let sqlite =
        include_str!("../../../migrations/sqlite/20260623000000_phase_13_command_results.sql");
    let postgres =
        include_str!("../../../migrations/postgres/20260623000000_phase_13_command_results.sql");

    assert_eq!(
        sqlite.trim(),
        "ALTER TABLE commands ADD COLUMN result_json TEXT;"
    );
    assert_eq!(postgres.trim(), sqlite.trim());
}

#[test]
fn phase_14_material_migrations_are_backend_equivalent() {
    let sqlite = include_str!("../../../migrations/sqlite/20260623010000_phase_14_materials.sql");
    let postgres =
        include_str!("../../../migrations/postgres/20260623010000_phase_14_materials.sql");

    assert_eq!(postgres.trim(), sqlite.trim());
    for required in [
        "ALTER TABLE jobs ADD COLUMN ams_mapping_json TEXT;",
        "ALTER TABLE jobs ADD COLUMN ams_mapping2_json TEXT;",
        "CREATE TABLE printer_material_snapshots",
        "ams_json TEXT NOT NULL",
        "external_spools_json TEXT NOT NULL",
        "active_tray_json TEXT",
        "UNIQUE (tenant_id, printer_id)",
        "idx_printer_material_snapshots_tenant_printer",
        "idx_printer_material_snapshots_tenant_serial",
        "CREATE TABLE job_filament_usages",
        "source TEXT NOT NULL CHECK (source IN ('ams_mapping2', 'ams_mapping'))",
        "confidence TEXT NOT NULL CHECK (confidence IN ('mapped_no_quantity', 'report_estimate'))",
        "UNIQUE (tenant_id, job_id, slot_index, source)",
        "idx_job_filament_usages_tenant_job",
        "idx_job_filament_usages_tenant_job_slot",
    ] {
        assert!(
            sqlite.contains(required),
            "missing migration fragment: {required}"
        );
    }
}

#[test]
fn filament_switch_state_migrations_are_backend_equivalent() {
    let sqlite =
        include_str!("../../../migrations/sqlite/20260716000000_filament_switch_state.sql");
    let postgres =
        include_str!("../../../migrations/postgres/20260716000000_filament_switch_state.sql");

    for migration in [sqlite, postgres] {
        assert!(migration.contains("ALTER TABLE printer_material_snapshots"));
        assert!(migration.contains("ADD COLUMN filament_switch_installed"));
    }
    assert!(sqlite.contains("INTEGER"));
    assert!(postgres.contains("BOOLEAN"));
}

#[test]
fn studio_status_flag_migrations_are_backend_equivalent() {
    let sqlite = include_str!("../../../migrations/sqlite/20260716010000_studio_status_flags.sql");
    let postgres =
        include_str!("../../../migrations/postgres/20260716010000_studio_status_flags.sql");

    assert_eq!(postgres.trim(), sqlite.trim());
    for column in ["studio_cfg", "studio_aux", "studio_stat"] {
        assert!(sqlite.contains(&format!(
            "ALTER TABLE printer_material_snapshots ADD COLUMN {column} TEXT;"
        )));
    }
}

#[test]
fn printer_access_code_encryption_migrations_are_backend_equivalent() {
    let sqlite =
        include_str!("../../../migrations/sqlite/20260718000000_encrypt_printer_access_codes.sql");
    let postgres = include_str!(
        "../../../migrations/postgres/20260718000000_encrypt_printer_access_codes.sql"
    );

    assert_eq!(postgres.trim(), sqlite.trim());
    assert_eq!(
        sqlite.trim(),
        "ALTER TABLE printers ADD COLUMN access_code_encrypted TEXT;"
    );
}

#[test]
fn mqtt_presence_session_migrations_are_backend_equivalent() {
    let sqlite =
        include_str!("../../../migrations/sqlite/20260720000000_mqtt_presence_session.sql");
    let postgres =
        include_str!("../../../migrations/postgres/20260720000000_mqtt_presence_session.sql");

    assert_eq!(postgres.trim(), sqlite.trim());
    assert_eq!(
        sqlite.trim(),
        "ALTER TABLE printers ADD COLUMN mqtt_presence_session_id TEXT;"
    );
}

#[test]
fn phase_28_slicer_metadata_migrations_are_backend_equivalent() {
    let sqlite =
        include_str!("../../../migrations/sqlite/20260624010000_phase_28_slicer_metadata.sql");
    let postgres =
        include_str!("../../../migrations/postgres/20260624010000_phase_28_slicer_metadata.sql");

    assert_eq!(postgres.trim(), sqlite.trim());
    assert_eq!(
        sqlite.trim(),
        "ALTER TABLE job_artifacts ADD COLUMN metadata_json TEXT;"
    );
}

#[tokio::test]
async fn agent_create_and_list_are_scoped_to_tenant() {
    let (_, tenants, agents, _, _, _) = repositories().await;
    let acme = tenants.create("acme", "Acme Labs").await.unwrap();
    let beta = tenants.create("beta", "Beta Labs").await.unwrap();

    let acme_agent = agents.create(acme.id, "shop-floor").await.unwrap();
    agents.create(beta.id, "shop-floor").await.unwrap();

    assert_eq!(agents.count().await.unwrap(), 2);
    assert_eq!(
        agents.list_for_tenant(acme.id).await.unwrap(),
        vec![acme_agent]
    );
}

#[tokio::test]
async fn agent_delete_offline_removes_agent_cascades_and_audits() {
    let (database, tenants, agents, printers, commands, _) = repositories().await;
    let tenant = tenants.create("delete-acme", "Delete Acme").await.unwrap();
    let admin = AuthRepository::new(database.clone())
        .create_user(
            tenant.id,
            "delete-admin@example.test",
            "Delete Admin",
            crate::repositories::UserRole::TenantAdmin,
        )
        .await
        .unwrap();
    let agent = agents.create(tenant.id, "stale-agent").await.unwrap();
    let printer_id = insert_printer_fixture(&database, tenant.id, agent.id)
        .await
        .unwrap();
    insert_command_fixture(&database, tenant.id, agent.id, Some(&printer_id))
        .await
        .unwrap();

    let deleted = agents
        .delete_offline_with_audit(
            tenant.id,
            agent.id,
            crate::repositories::AuditActor::user(admin.id.clone()),
        )
        .await
        .unwrap();

    assert_eq!(deleted, agent);
    assert_eq!(agents.get(agent.id).await.unwrap(), None);
    assert_eq!(printers.count().await.unwrap(), 0);
    assert_eq!(commands.count().await.unwrap(), 0);

    let audit = AuditEventRepository::new(database);
    let events = audit.list_for_tenant(tenant.id).await.unwrap();
    let event = events
        .iter()
        .find(|event| event.action == "agent.delete")
        .expect("agent delete audit event");
    assert_eq!(event.target_type, "agent");
    assert_eq!(
        event.target_id.as_deref(),
        Some(agent.id.to_string().as_str())
    );
    let metadata: TestAgentDeleteAuditMetadata =
        serde_json::from_str(&event.metadata_json).unwrap();
    assert_eq!(
        metadata,
        TestAgentDeleteAuditMetadata {
            agent_name: "stale-agent".to_owned(),
            previous_status: "offline".to_owned(),
        }
    );
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct TestAgentDeleteAuditMetadata {
    agent_name: String,
    previous_status: String,
}
