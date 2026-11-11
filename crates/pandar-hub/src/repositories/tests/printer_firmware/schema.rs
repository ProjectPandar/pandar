use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use super::*;
use crate::entities::printers;

const SQLITE_MIGRATION_PATH: &str = "migrations/sqlite/20260711010000_printer_firmware.sql";
const POSTGRES_MIGRATION_PATH: &str = "migrations/postgres/20260711010000_printer_firmware.sql";

#[test]
fn firmware_migration_sqlite_and_postgres_differ_only_by_integer_spelling() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let sqlite = std::fs::read_to_string(root.join(SQLITE_MIGRATION_PATH)).unwrap();
    let postgres = std::fs::read_to_string(root.join(POSTGRES_MIGRATION_PATH)).unwrap();

    assert_eq!(sqlite, postgres.replace("BIGINT", "INTEGER"));
}

#[tokio::test]
async fn firmware_migration_defaults_and_nonnegative_checks_hold_in_sqlite() {
    let fixture = FirmwareFixture::new(sqlite_database().await, "firmware-schema").await;
    let stored = printers::Entity::find_by_id(&fixture.printer_id)
        .one(&fixture.database.sea_orm_connection())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.firmware_modules_json, None);
    assert_eq!(stored.firmware_upgrade_state_json, None);
    assert_eq!(stored.firmware_cfg, None);
    assert_eq!(stored.firmware_session_id, None);
    assert_eq!(stored.firmware_generation, None);
    assert_eq!(stored.firmware_module_revision, 0);
    assert_eq!(stored.firmware_status_revision, 0);

    let Database::Sqlite(pool) = &fixture.database else {
        panic!("expected SQLite database");
    };
    let invalid_module_revision =
        sqlx::query("UPDATE printers SET firmware_module_revision = -1 WHERE id = ?1")
            .bind(&fixture.printer_id)
            .execute(pool)
            .await;
    assert!(invalid_module_revision.is_err());
    let invalid_status_revision =
        sqlx::query("UPDATE printers SET firmware_status_revision = -1 WHERE id = ?1")
            .bind(&fixture.printer_id)
            .execute(pool)
            .await;
    assert!(invalid_status_revision.is_err());

    let listed = printers::Entity::find()
        .filter(printers::Column::TenantId.eq(fixture.tenant_id.to_string()))
        .all(&fixture.database.sea_orm_connection())
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
}
