use super::*;

#[tokio::test]
async fn personal_preset_inner_failure_rolls_back_once_and_preserves_the_cause() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let presets = PersonalPresetRepository::new(database.clone());
    let tenant = tenants.create("preset-rollback", "Presets").await.unwrap();
    let user = auth
        .create_user(tenant.id, "owner@test", "Owner", UserRole::Operator)
        .await
        .unwrap();
    let Database::Sqlite(pool) = &database else {
        panic!("expected SQLite database");
    };
    sqlx::query(
        "CREATE TRIGGER fail_personal_preset_audit \
         BEFORE INSERT ON audit_events \
         WHEN NEW.action = 'personal_preset.create' \
         BEGIN SELECT RAISE(ABORT, 'forced audit failure'); END",
    )
    .execute(pool)
    .await
    .unwrap();

    let error = presets
        .create_with_audit(
            tenant.id,
            &user.id,
            input(PersonalPresetType::Print, "Rolled Back"),
            AuditActor::user(&user.id),
        )
        .await
        .unwrap_err();
    let error = format!("{error:#}");
    assert!(error.contains("failed to insert audit event"), "{error}");
    assert!(error.contains("forced audit failure"), "{error}");
    assert!(
        presets
            .list_metadata(tenant.id, &user.id)
            .await
            .unwrap()
            .is_empty()
    );

    sqlx::query("DROP TRIGGER fail_personal_preset_audit")
        .execute(pool)
        .await
        .unwrap();
    presets
        .create_with_audit(
            tenant.id,
            &user.id,
            input(PersonalPresetType::Print, "Rolled Back"),
            AuditActor::user(&user.id),
        )
        .await
        .unwrap();
}
