use super::*;

#[tokio::test]
async fn external_identity_rejects_missing_and_duplicate_links() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database);
    let tenant = tenants
        .create("acme-identity-duplicates", "Acme Identity")
        .await
        .unwrap();
    let user = auth
        .create_user(
            tenant.id,
            "admin@example.test",
            "Admin",
            UserRole::TenantAdmin,
        )
        .await
        .unwrap();

    let missing = auth
        .link_external_identity(tenant.id, "missing-user", "clerk", "user_missing")
        .await
        .unwrap_err();
    assert!(matches!(missing, RepositoryError::MissingUser));

    auth.link_external_identity(tenant.id, &user.id, "clerk", "user_123")
        .await
        .unwrap();

    let duplicate_identity = auth
        .link_external_identity(tenant.id, &user.id, "clerk", "user_123")
        .await
        .unwrap_err();
    assert!(matches!(
        duplicate_identity,
        RepositoryError::DuplicateExternalIdentity
    ));

    let duplicate_user_provider = auth
        .link_external_identity(tenant.id, &user.id, "clerk", "user_456")
        .await
        .unwrap_err();
    assert!(matches!(
        duplicate_user_provider,
        RepositoryError::DuplicateUserExternalIdentity
    ));
}
