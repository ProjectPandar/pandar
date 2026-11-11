use super::*;

#[tokio::test]
async fn last_tenant_admin_cannot_be_demoted() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database);
    let tenant = tenants
        .create("acme-demote-admin", "Acme Demote Admin")
        .await
        .unwrap();
    let admin = auth
        .create_user(
            tenant.id,
            "admin@example.test",
            "Admin",
            UserRole::TenantAdmin,
        )
        .await
        .unwrap();

    let err = auth
        .update_user_role_with_audit(
            tenant.id,
            &admin.id,
            UserRole::Viewer,
            AuditActor::user(admin.id.clone()),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, RepositoryError::LastTenantAdmin));
    assert_eq!(
        auth.list_users_for_tenant(tenant.id).await.unwrap()[0].role,
        UserRole::TenantAdmin
    );
}

#[tokio::test]
async fn duplicate_user_email_is_reported() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database);
    let acme = tenants
        .create("acme-duplicate-email", "Acme Duplicate Email")
        .await
        .unwrap();
    let beta = tenants
        .create("beta-duplicate-email", "Beta Duplicate Email")
        .await
        .unwrap();

    auth.create_user(acme.id, "user@example.test", "User", UserRole::Viewer)
        .await
        .unwrap();
    let duplicate = auth
        .create_user(acme.id, "user@example.test", "Other", UserRole::Operator)
        .await
        .unwrap_err();
    assert!(matches!(duplicate, RepositoryError::DuplicateUserEmail));

    auth.create_user(beta.id, "user@example.test", "User", UserRole::Viewer)
        .await
        .unwrap();
}
