use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};

use super::{AuthRepository, TenantRepository, sqlite_database};

#[test]
fn phase_16_migrations_are_backend_equivalent() {
    let sqlite = include_str!(
        "../../../migrations/sqlite/20260623020000_phase_16_tenant_tokens_agent_credentials.sql"
    );
    let postgres = include_str!(
        "../../../migrations/postgres/20260623020000_phase_16_tenant_tokens_agent_credentials.sql"
    );

    assert_eq!(sqlite, postgres);
    assert!(!sqlite.contains("UNIQUE(tenant_id, name)"));
    assert!(!sqlite.contains("UNIQUE (tenant_id, name)"));
}

#[tokio::test]
async fn tenant_tokens_create_authenticate_update_last_used_rotate_and_revoke() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database);
    let tenant = tenants.create("tokens", "Tokens").await.unwrap();
    let admin = auth
        .create_user(
            tenant.id,
            "admin@example.test",
            "Admin",
            crate::repositories::UserRole::TenantAdmin,
        )
        .await
        .unwrap();

    let created = auth
        .create_tenant_token_with_audit(
            tenant.id,
            "Studio",
            vec![
                crate::repositories::TenantTokenScope::All,
                crate::repositories::TenantTokenScope::AgentRegister,
            ],
            None,
            crate::repositories::AuditActor::user(admin.id.clone()),
        )
        .await
        .unwrap();
    assert_eq!(created.token.created_by_user_id, Some(admin.id.clone()));
    assert_eq!(
        created.token.scopes,
        vec![
            crate::repositories::TenantTokenScope::All,
            crate::repositories::TenantTokenScope::AgentRegister
        ]
    );
    assert!(created.plaintext_token.starts_with("pandar_tenant_"));
    assert_ne!(created.plaintext_token, "tenant-token-secret");

    let stored = auth.list_tenant_tokens(tenant.id).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].last_used_at, None);

    let authenticated = auth
        .authenticate_tenant_token(&created.plaintext_token)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(authenticated.token.id, created.token.id);
    assert_eq!(authenticated.token.tenant_id, tenant.id);
    assert!(authenticated.token.last_used_at.is_none());

    let stored = auth.list_tenant_tokens(tenant.id).await.unwrap();
    let last_used_at = stored[0].last_used_at.clone().unwrap();

    let rotated = auth
        .rotate_tenant_token_with_audit(
            tenant.id,
            &created.token.id,
            None,
            crate::repositories::AuditActor::user(admin.id),
        )
        .await
        .unwrap();
    assert!(rotated.plaintext_token.starts_with("pandar_tenant_"));
    assert_ne!(rotated.token.id, created.token.id);
    assert_eq!(rotated.token.name, created.token.name);
    assert_eq!(rotated.token.scopes, created.token.scopes);
    assert_eq!(rotated.token.last_used_at, None);
    assert!(
        auth.authenticate_tenant_token(&created.plaintext_token)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        auth.authenticate_tenant_token(&rotated.plaintext_token)
            .await
            .unwrap()
            .is_some()
    );
    let stored = auth.list_tenant_tokens(tenant.id).await.unwrap();
    let old = stored
        .iter()
        .find(|token| token.id == created.token.id)
        .unwrap();
    assert!(old.revoked_at.is_some());
    assert_eq!(old.last_used_at.as_deref(), Some(last_used_at.as_str()));

    let revoked = auth
        .revoke_tenant_token_with_audit(
            tenant.id,
            &rotated.token.id,
            crate::repositories::AuditActor::tenant_token(
                None,
                rotated.token.id.clone(),
                vec!["*"],
            ),
        )
        .await
        .unwrap();
    assert!(revoked.revoked_at.is_some());
    assert!(
        auth.authenticate_tenant_token(&rotated.plaintext_token)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn mobile_login_ticket_exchange_is_one_use_and_creates_tenant_token() {
    let database = sqlite_database().await;
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let tenant = tenants
        .create("mobile-login", "Mobile Login")
        .await
        .unwrap();
    let admin = auth
        .create_user(
            tenant.id,
            "mobile-admin@example.test",
            "Mobile Admin",
            crate::repositories::UserRole::TenantAdmin,
        )
        .await
        .unwrap();
    let ticket = auth
        .create_mobile_login_ticket_with_audit(
            tenant.id,
            Some(admin.id.clone()),
            "zip.iptables.pandar.android://auth/callback",
            "ZtNPunH49FD35FWYhT5Tv8I7vRKQJ8uxMaL0_9eHjNA".to_owned(),
            future_rfc3339(),
            crate::repositories::AuditActor::user(admin.id.clone()),
        )
        .await
        .unwrap();

    assert_eq!(
        ticket.ticket.redirect_url,
        "zip.iptables.pandar.android://auth/callback"
    );

    let exchanged = auth
        .exchange_mobile_login_ticket(
            &ticket.plaintext_ticket,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(exchanged.redirect_url, ticket.ticket.redirect_url);
    assert!(
        exchanged
            .tenant_token
            .plaintext_token
            .starts_with("pandar_mobile_")
    );
    assert_eq!(exchanged.tenant_token.token.tenant_id, tenant.id);
    assert_eq!(
        exchanged.tenant_token.token.created_by_user_id,
        Some(admin.id)
    );
    assert_eq!(
        exchanged.tenant_token.token.scopes,
        vec![crate::repositories::TenantTokenScope::MobileSession]
    );

    assert!(
        auth.exchange_mobile_login_ticket(
            &ticket.plaintext_ticket,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .unwrap()
        .is_none()
    );
}

#[tokio::test]
async fn mobile_redirect_validation_allows_only_android_callback() {
    let auth = AuthRepository::new(sqlite_database().await);
    assert_eq!(
        auth.validate_mobile_redirect_url("zip.iptables.pandar.android://auth/callback")
            .unwrap(),
        "zip.iptables.pandar.android://auth/callback"
    );

    for invalid in [
        "zip.iptables.pandar.android:/oauth2redirect",
        "zip.iptables.pandar.android:/auth/callback",
        "zip.iptables.pandar.android://auth/callback?state=1",
        "zip.iptables.pandar.android://auth/callback#fragment",
        "http://localhost:4100/callback",
        "https://pandar.example/mobile",
    ] {
        assert!(
            auth.validate_mobile_redirect_url(invalid).is_err(),
            "accepted invalid mobile redirect {invalid}"
        );
    }
}

#[tokio::test]
async fn plugin_redirect_validation_allows_localhost_http_with_path_and_query_only() {
    let auth = AuthRepository::new(sqlite_database().await);
    assert_eq!(
        auth.validate_plugin_redirect_url("http://localhost:3000/callback?code=1")
            .unwrap(),
        "http://localhost:3000/callback?code=1"
    );
    assert_eq!(
        auth.validate_plugin_redirect_url("http://127.0.0.1:1/")
            .unwrap(),
        "http://127.0.0.1:1/"
    );
    assert_eq!(
        auth.validate_plugin_redirect_url("http://[::1]:65535/plugin")
            .unwrap(),
        "http://[::1]:65535/plugin"
    );

    for invalid in [
        "https://localhost:3000/callback",
        "http://example.test:3000/callback",
        "http://localhost/callback",
        "http://localhost:0/callback",
        "http://localhost:65536/callback",
        "http://user@localhost:3000/callback",
        "http://localhost:3000/callback#fragment",
    ] {
        assert!(auth.validate_plugin_redirect_url(invalid).is_err());
    }
}

fn future_rfc3339() -> String {
    (OffsetDateTime::now_utc() + Duration::minutes(5))
        .format(&Rfc3339)
        .unwrap()
}
