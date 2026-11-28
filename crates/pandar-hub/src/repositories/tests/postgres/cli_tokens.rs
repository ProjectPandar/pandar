use super::*;
use crate::repositories::{AuditActor, TenantTokenScope, UserRole};

const CODE_VERIFIER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CODE_CHALLENGE: &str = "ZtNPunH49FD35FWYhT5Tv8I7vRKQJ8uxMaL0_9eHjNA";

#[tokio::test]
async fn postgres_cli_login_tickets_concurrent_exchange_and_revocation() {
    let Some(database) = postgres_database().await else {
        eprintln!("skipping PostgreSQL test; PANDAR_TEST_POSTGRES_URL is not set");
        return;
    };
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let tenant = tenants.create("pg-cli", "PG CLI").await.unwrap();
    let user = auth
        .create_user(tenant.id, "pg@example.test", "PG User", UserRole::Operator)
        .await
        .unwrap();

    let created = auth
        .create_cli_login_ticket_with_audit(
            tenant.id,
            Some(user.id.clone()),
            "http://127.0.0.1:54321/callback",
            CODE_CHALLENGE.to_owned(),
            "2099-01-01T00:00:00Z".to_owned(),
            AuditActor::user(user.id.clone()),
        )
        .await
        .unwrap();

    let mut handles = Vec::new();
    for _ in 0..8 {
        let auth = auth.clone();
        let ticket = created.plaintext_ticket.clone();
        handles.push(tokio::spawn(async move {
            auth.exchange_cli_login_ticket(&ticket, CODE_VERIFIER).await
        }));
    }
    let mut winners = Vec::new();
    for handle in handles {
        if let Some(exchanged) = handle.await.unwrap().unwrap() {
            winners.push(exchanged);
        }
    }
    assert_eq!(winners.len(), 1);
    let session_token = winners.pop().unwrap().tenant_token.plaintext_token;

    let (r1, r2) = tokio::join!(
        auth.revoke_cli_session_token_with_audit(&session_token),
        auth.revoke_cli_session_token_with_audit(&session_token),
    );
    assert!(r1.unwrap().is_some());
    assert!(r2.unwrap().is_some());
    assert!(
        auth.authenticate_tenant_token(&session_token)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn postgres_tenant_tokens_operator_viewer_normalization_and_legacy_read() {
    let Some(database) = postgres_database().await else {
        eprintln!("skipping PostgreSQL test; PANDAR_TEST_POSTGRES_URL is not set");
        return;
    };
    let tenants = TenantRepository::new(database.clone());
    let auth = AuthRepository::new(database.clone());
    let tenant = tenants.create("pg-tok", "PG Tokens").await.unwrap();

    let created = auth
        .create_tenant_token_with_audit(
            tenant.id,
            "normalized",
            vec![
                TenantTokenScope::Viewer,
                TenantTokenScope::Operator,
                TenantTokenScope::Viewer,
            ],
            None,
            AuditActor::no_auth(),
        )
        .await
        .unwrap();
    assert_eq!(
        created.token.scopes,
        vec![TenantTokenScope::Operator, TenantTokenScope::Viewer]
    );
    let loaded = auth
        .authenticate_tenant_token(&created.plaintext_token)
        .await
        .unwrap()
        .expect("normalized token should authenticate");
    assert_eq!(
        loaded.token.scopes,
        vec![TenantTokenScope::Operator, TenantTokenScope::Viewer]
    );
}
