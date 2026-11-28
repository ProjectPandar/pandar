use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde::Deserialize;

use super::*;
use crate::{
    entities::audit_events,
    repositories::{TenantTokenScope, UserRole},
};

const CALLBACK: &str = "http://127.0.0.1:54321/callback";
const CODE_VERIFIER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CODE_CHALLENGE: &str = "ZtNPunH49FD35FWYhT5Tv8I7vRKQJ8uxMaL0_9eHjNA";
const EXCHANGE_URI: &str = "/api/v1/cli/login-tickets/exchange";
const SESSION_URI: &str = "/api/v1/cli/session";
const WHOAMI_URI: &str = "/api/v1/auth/whoami";

#[derive(Serialize)]
struct CliLoginTicketRequest<'a> {
    redirect_url: &'a str,
    code_challenge: &'a str,
}

#[derive(Serialize)]
struct CliTicketExchangeRequest<'a> {
    ticket: &'a str,
    code_verifier: &'a str,
}

#[derive(Serialize)]
struct CreateTokenBody<'a> {
    name: &'a str,
    scopes: &'a [&'a str],
}

#[derive(Serialize)]
struct UpdateTenantBody<'a> {
    display_name: &'a str,
}

#[derive(Serialize)]
struct JoinLinkBody<'a> {
    role: &'a str,
}

#[derive(Serialize)]
struct PrinterControlBody<'a> {
    action: &'a str,
    light_on: bool,
}

#[derive(Debug, Deserialize)]
struct LoginTicketResponse {
    ticket: String,
    expires_at: String,
    redirect_url: String,
}

#[derive(Debug, Deserialize)]
struct ExchangeResponse {
    token: String,
    expires_at: String,
    profile: Profile,
}

#[derive(Debug, Deserialize)]
struct Profile {
    user_id: String,
    user_name: String,
    tenant_id: String,
    tenant_name: String,
}

#[derive(Debug, Deserialize)]
struct WhoamiResponse {
    token_id: String,
    token_name: String,
    tenant_id: String,
    tenant_slug: String,
    tenant_display_name: String,
    scopes: Vec<String>,
    effective_role: Option<String>,
    expires_at: Option<String>,
    user: Option<WhoamiUser>,
}

#[derive(Debug, Deserialize)]
struct WhoamiUser {
    id: String,
    email: String,
    display_name: String,
    role: String,
}

#[derive(Debug, Deserialize)]
struct TenantTokenSecretResponse {
    tenant_token: TenantTokenSummary,
    token: String,
}

#[derive(Debug, Deserialize)]
struct TenantTokenSummary {
    scopes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

fn tickets_uri(tenant_id: TenantId) -> String {
    format!("/api/v1/tenants/{tenant_id}/cli/login-tickets")
}

fn ticket_body(redirect_url: &str) -> Option<Value> {
    Some(json_body(CliLoginTicketRequest {
        redirect_url,
        code_challenge: CODE_CHALLENGE,
    }))
}

fn exchange_body(ticket: &str, code_verifier: &str) -> Option<Value> {
    Some(json_body(CliTicketExchangeRequest {
        ticket,
        code_verifier,
    }))
}

#[tokio::test]
async fn whoami_validates_bearer_tenant_tokens_and_bypasses_no_auth() {
    let state = external_auth_state(state().await).with_no_auth_for_tests(true);
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("whoami-tenant", "Whoami Tenant")
        .await
        .unwrap();
    let admin_jwt =
        external_auth_token_for_role(&state, tenant.id, UserRole::TenantAdmin, "whoami-jwt").await;

    // Missing header must fail even under no-auth.
    let (status, body) = request(app.clone(), Method::GET, WHOAMI_URI, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        decode_json::<ErrorResponse>(body).error,
        "missing_auth_token"
    );

    // Invalid token & external JWT must both fail with 401 even under no-auth.
    for bad in ["not-a-token", admin_jwt.as_str()] {
        let (status, body) = request_as(app.clone(), Method::GET, WHOAMI_URI, None, bad).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(
            decode_json::<ErrorResponse>(body).error,
            "invalid_auth_token"
        );
    }

    let actor = crate::repositories::AuditActor::no_auth();
    let cases = [
        (vec![TenantTokenScope::All], Some("tenant_admin"), vec!["*"]),
        (
            vec![TenantTokenScope::Operator],
            Some("operator"),
            vec!["operator"],
        ),
        (
            vec![TenantTokenScope::Viewer],
            Some("viewer"),
            vec!["viewer"],
        ),
        (vec![], Some("viewer"), vec![]),
        (
            vec![TenantTokenScope::AgentRegister],
            None,
            vec!["agent:register"],
        ),
    ];

    for (index, (scopes, expected_role, expected_scopes)) in cases.into_iter().enumerate() {
        let created = state
            .auth()
            .create_tenant_token_with_audit(
                tenant.id,
                format!("key-{index}"),
                scopes,
                None,
                actor.clone(),
            )
            .await
            .unwrap();
        let (status, body) = request_as(
            app.clone(),
            Method::GET,
            WHOAMI_URI,
            None,
            &created.plaintext_token,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let whoami = decode_json::<WhoamiResponse>(body);
        assert_eq!(whoami.token_id, created.token.id);
        assert_eq!(whoami.token_name, format!("key-{index}"));
        assert_eq!(whoami.tenant_id, tenant.id.to_string());
        assert_eq!(whoami.tenant_slug, "whoami-tenant");
        assert_eq!(whoami.tenant_display_name, "Whoami Tenant");
        assert_eq!(whoami.effective_role.as_deref(), expected_role);
        assert_eq!(whoami.scopes, expected_scopes);
        assert_eq!(whoami.expires_at, None);
        assert!(whoami.user.is_none());
    }
}

#[tokio::test]
async fn api_key_scope_matrix_normalization_and_out_of_scope_boundaries() {
    let state = external_auth_state(state().await);
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("matrix-tenant", "Matrix Tenant")
        .await
        .unwrap();
    let admin_jwt =
        external_auth_token_for_role(&state, tenant.id, UserRole::TenantAdmin, "matrix-admin")
            .await;
    let agent = state
        .agents()
        .create(tenant.id, "shop-agent")
        .await
        .unwrap();
    let printer_id = insert_printer_fixture(state.database(), tenant.id, agent.id)
        .await
        .unwrap();

    // Normalization: duplicates removed and sorted canonically (* -> operator -> viewer -> agent:register -> plugin:studio).
    let (status, body) = request_as(
        app.clone(),
        Method::POST,
        &format!("/api/v1/tenants/{}/tenant-tokens", tenant.id),
        Some(json_body(CreateTokenBody {
            name: "op-normalized",
            scopes: &["viewer", "operator", "viewer"],
        })),
        &admin_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let op_created = decode_json::<TenantTokenSecretResponse>(body);
    assert_eq!(op_created.tenant_token.scopes, vec!["operator", "viewer"]);
    let op_key = op_created.token;

    let (status, body) = request_as(
        app.clone(),
        Method::POST,
        &format!("/api/v1/tenants/{}/tenant-tokens", tenant.id),
        Some(json_body(CreateTokenBody {
            name: "viewer-key",
            scopes: &["viewer"],
        })),
        &admin_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let viewer_key = decode_json::<TenantTokenSecretResponse>(body).token;

    let (status, body) = request_as(
        app.clone(),
        Method::POST,
        &format!("/api/v1/tenants/{}/tenant-tokens", tenant.id),
        Some(json_body(CreateTokenBody {
            name: "all-key",
            scopes: &["operator", "*"],
        })),
        &admin_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let all_created = decode_json::<TenantTokenSecretResponse>(body);
    assert_eq!(all_created.tenant_token.scopes, vec!["*", "operator"]);
    let all_key = all_created.token;

    // Read routes: viewer, operator, all succeed.
    for key in [&viewer_key, &op_key, &all_key] {
        for uri in [
            format!("/api/v1/tenants/{}/printers", tenant.id),
            format!("/api/v1/tenants/{}/printers/{printer_id}", tenant.id),
            format!("/api/v1/tenants/{}/jobs", tenant.id),
            format!("/api/v1/tenants/{}/agents", tenant.id),
        ] {
            let (status, _) = request_as(app.clone(), Method::GET, &uri, None, key).await;
            assert_eq!(status, StatusCode::OK, "GET {uri}");
        }
    }

    // Operator routes: viewer gets 403; operator passes auth and succeeds.
    let refresh_uri = format!(
        "/api/v1/tenants/{}/printers/{printer_id}/materials:refresh",
        tenant.id
    );
    let (status, _) = request_as(app.clone(), Method::POST, &refresh_uri, None, &viewer_key).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, body) = request_as(app.clone(), Method::POST, &refresh_uri, None, &op_key).await;
    assert!(
        status.is_success(),
        "operator materials:refresh failed with {status}: {body}"
    );
    let control_uri = format!(
        "/api/v1/tenants/{}/printers/{printer_id}/controls",
        tenant.id
    );
    let (status, _) = request_as(
        app.clone(),
        Method::POST,
        &control_uri,
        Some(json_body(PrinterControlBody {
            action: "set_chamber_light",
            light_on: true,
        })),
        &viewer_key,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request_as(
        app.clone(),
        Method::POST,
        &control_uri,
        Some(json_body(PrinterControlBody {
            action: "set_chamber_light",
            light_on: true,
        })),
        &op_key,
    )
    .await;
    assert_ne!(status, StatusCode::FORBIDDEN);
    assert_ne!(status, StatusCode::UNAUTHORIZED);

    let jobs_uri = format!("/api/v1/tenants/{}/printers/{printer_id}/jobs", tenant.id);
    let plate = || multipart_print_body(None, Some(("plate.3mf", "model/3mf", b"abc")), 1);
    let (status, _) =
        multipart_request_as(app.clone(), Method::POST, &jobs_uri, plate(), &viewer_key).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) =
        multipart_request_as(app.clone(), Method::POST, &jobs_uri, plate(), &op_key).await;
    assert_eq!(status, StatusCode::CREATED);

    // Tenant admin routes: operator gets 403; all succeeds.
    for (method, uri, body) in [
        (
            Method::GET,
            format!("/api/v1/tenants/{}/tenant-tokens", tenant.id),
            None,
        ),
        (
            Method::GET,
            format!("/api/v1/tenants/{}/users", tenant.id),
            None,
        ),
        (
            Method::GET,
            format!("/api/v1/tenants/{}/audit-events", tenant.id),
            None,
        ),
        (
            Method::DELETE,
            format!("/api/v1/tenants/{}/printers/{printer_id}", tenant.id),
            None,
        ),
    ] {
        let (status, _) =
            request_as(app.clone(), method.clone(), &uri, body.clone(), &op_key).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "operator {method} {uri}");
        let (status, _) = request_as(app.clone(), method.clone(), &uri, body, &all_key).await;
        assert_eq!(status, StatusCode::OK, "all {method} {uri}");
    }

    // External-user-only routes reject ALL tenant tokens (including `*`).
    for (method, uri, body) in [
        (
            Method::PATCH,
            format!("/api/v1/tenants/{}", tenant.id),
            Some(json_body(UpdateTenantBody {
                display_name: "Renamed",
            })),
        ),
        (
            Method::POST,
            format!("/api/v1/tenants/{}/join-links", tenant.id),
            Some(json_body(JoinLinkBody { role: "viewer" })),
        ),
        (Method::POST, tickets_uri(tenant.id), ticket_body(CALLBACK)),
    ] {
        let (status, _) = request_as(app.clone(), method.clone(), &uri, body, &all_key).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "tenant token {method} {uri}");
    }

    // Out-of-scope auth boundaries reject tenant tokens: bootstrap, external /me, plugin-only, agent-credential.
    let (status, _) = request_as(app.clone(), Method::GET, "/api/v1/tenants", None, &all_key).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = request_as(app.clone(), Method::GET, "/api/v1/me", None, &all_key).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = request_as(
        app.clone(),
        Method::GET,
        "/api/v1/plugin/printers",
        None,
        &all_key,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _) = request_as(
        app,
        Method::GET,
        &format!("/api/v1/agents/{}/printers", agent.id),
        None,
        &all_key,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cli_login_ticket_lifecycle_pkce_downgrade_deletion_and_idempotent_logout() {
    let state = external_auth_state(state().await);
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("cli-tenant", "CLI Tenant")
        .await
        .unwrap();
    let other_tenant = state
        .tenants()
        .create("cli-other", "CLI Other")
        .await
        .unwrap();
    let viewer_jwt =
        external_auth_token_for_role(&state, tenant.id, UserRole::Viewer, "cli-viewer").await;
    let operator_jwt =
        external_auth_token_for_role(&state, tenant.id, UserRole::Operator, "cli-operator").await;

    // No-auth mode rejects CLI login ticket creation with 403 external_auth_required.
    let no_auth_app = router(state.clone().with_no_auth_for_tests(true));
    let (status, body) = request(
        no_auth_app.clone(),
        Method::POST,
        &tickets_uri(tenant.id),
        ticket_body(CALLBACK),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(
        decode_json::<ErrorResponse>(body).error,
        "external_auth_required"
    );

    // Cross-tenant is forbidden.
    let (status, _) = request_as(
        app.clone(),
        Method::POST,
        &tickets_uri(other_tenant.id),
        ticket_body(CALLBACK),
        &viewer_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Redirect validation: accept loopback http with non-zero port, reject everything else.
    for accepted in [
        CALLBACK,
        "http://localhost:8081/callback",
        "http://[::1]:9000/cb",
    ] {
        let (status, body) = request_as(
            app.clone(),
            Method::POST,
            &tickets_uri(tenant.id),
            ticket_body(accepted),
            &viewer_jwt,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{accepted} rejected: {body}");
    }
    for rejected in [
        "https://127.0.0.1:54321/callback",
        "http://evil.example:54321/callback",
        "http://127.0.0.1/callback",
        "http://127.0.0.1:0/callback",
        "http://user@127.0.0.1:54321/callback",
        "http://127.0.0.1:54321/callback?next=/",
        "http://127.0.0.1:54321/callback#frag",
        "zip.iptables.pandar.android://auth/callback",
    ] {
        let (status, body) = request_as(
            app.clone(),
            Method::POST,
            &tickets_uri(tenant.id),
            ticket_body(rejected),
            &viewer_jwt,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected} accepted");
        assert_eq!(
            decode_json::<ErrorResponse>(body).error,
            "invalid_redirect_url"
        );
    }

    // Create ticket as Operator and test wrong PKCE, cross-kind exchange, and valid one-use exchange.
    let (status, created) = request_as(
        app.clone(),
        Method::POST,
        &tickets_uri(tenant.id),
        ticket_body(CALLBACK),
        &operator_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = decode_json::<LoginTicketResponse>(created);
    assert!(created.ticket.starts_with("pandar_cli_ticket_"));
    assert!(created.expires_at.ends_with('Z'));
    assert_eq!(created.redirect_url, CALLBACK);

    let (status, _) = request(
        app.clone(),
        Method::POST,
        "/api/v1/mobile/login-tickets/exchange",
        exchange_body(&created.ticket, CODE_VERIFIER),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, _) = request(
        app.clone(),
        Method::POST,
        EXCHANGE_URI,
        exchange_body(
            &created.ticket,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, exchanged) = request(
        app.clone(),
        Method::POST,
        EXCHANGE_URI,
        exchange_body(&created.ticket, CODE_VERIFIER),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let exchanged = decode_json::<ExchangeResponse>(exchanged);
    assert!(exchanged.token.starts_with("pandar_cli_"));
    assert!(exchanged.expires_at.ends_with('Z'));
    assert_eq!(exchanged.profile.user_name, "External Test User");
    assert_eq!(exchanged.profile.tenant_id, tenant.id.to_string());
    assert_eq!(exchanged.profile.tenant_name, "CLI Tenant");

    // Second exchange fails (one-use).
    let (status, _) = request(
        app.clone(),
        Method::POST,
        EXCHANGE_URI,
        exchange_body(&created.ticket, CODE_VERIFIER),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Whoami returns live user and effective role.
    let (status, body) =
        request_as(app.clone(), Method::GET, WHOAMI_URI, None, &exchanged.token).await;
    assert_eq!(status, StatusCode::OK);
    let whoami = decode_json::<WhoamiResponse>(body);
    assert_eq!(whoami.scopes, vec!["cli:session"]);
    assert_eq!(whoami.effective_role.as_deref(), Some("operator"));
    let user_info = whoami.user.expect("cli session must expose user");
    assert_eq!(user_info.id, exchanged.profile.user_id);
    assert_eq!(user_info.email, "cli-operator@example.test");
    assert_eq!(user_info.display_name, "External Test User");
    assert_eq!(user_info.role, "operator");

    // Downgrade user to Viewer -> whoami & operator writes immediately reflect Viewer.
    state
        .auth()
        .update_user_role(tenant.id, &exchanged.profile.user_id, UserRole::Viewer)
        .await
        .unwrap();
    let (status, body) =
        request_as(app.clone(), Method::GET, WHOAMI_URI, None, &exchanged.token).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        decode_json::<WhoamiResponse>(body)
            .effective_role
            .as_deref(),
        Some("viewer")
    );

    // DELETE /api/v1/cli/session rejects API keys and bypasses no-auth, but idempotently revokes cli:session.
    let api_key = all_scope_tenant_token(&state, &tenant.id.to_string(), "keep-alive").await;
    let (status, _) = request(no_auth_app.clone(), Method::DELETE, SESSION_URI, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = request_as(
        no_auth_app.clone(),
        Method::DELETE,
        SESSION_URI,
        None,
        &api_key,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Concurrent + repeated logout both return 204 and emit exactly 1 audit event.
    let delete_session = |router_app: Router, bearer: String| async move {
        router_app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(SESSION_URI)
                    .header(AUTHORIZATION, format!("Bearer {bearer}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    };
    let (r1, r2) = tokio::join!(
        delete_session(app.clone(), exchanged.token.clone()),
        delete_session(no_auth_app, exchanged.token.clone()),
    );
    assert_eq!(r1, StatusCode::NO_CONTENT);
    assert_eq!(r2, StatusCode::NO_CONTENT);
    let revoke_events = audit_events::Entity::find()
        .filter(audit_events::Column::TenantId.eq(tenant.id.to_string()))
        .filter(audit_events::Column::Action.eq("tenant_token.revoke"))
        .all(&state.database().sea_orm_connection())
        .await
        .unwrap();
    assert_eq!(revoke_events.len(), 1);

    // Revoked CLI token now fails whoami with 401.
    let (status, _) =
        request_as(app.clone(), Method::GET, WHOAMI_URI, None, &exchanged.token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Deleting the owning user invalidates an active CLI session everywhere (including whoami -> 401).
    let (status, created2) = request_as(
        app.clone(),
        Method::POST,
        &tickets_uri(tenant.id),
        ticket_body(CALLBACK),
        &viewer_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ticket2 = decode_json::<LoginTicketResponse>(created2).ticket;
    let (status, exchanged2) = request(
        app.clone(),
        Method::POST,
        EXCHANGE_URI,
        exchange_body(&ticket2, CODE_VERIFIER),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let session2 = decode_json::<ExchangeResponse>(exchanged2);
    state
        .auth()
        .remove_user_with_audit(
            tenant.id,
            &session2.profile.user_id,
            crate::repositories::AuditActor::no_auth(),
        )
        .await
        .unwrap();
    let (status, body) =
        request_as(app.clone(), Method::GET, WHOAMI_URI, None, &session2.token).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        decode_json::<ErrorResponse>(body).error,
        "invalid_auth_token"
    );
    let (status, _) = request_as(
        app,
        Method::GET,
        &format!("/api/v1/tenants/{}/printers", tenant.id),
        None,
        &session2.token,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn cli_login_ticket_concurrent_exchange_allows_only_one_winner() {
    let state = external_auth_state(state().await);
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("cli-race", "CLI Race")
        .await
        .unwrap();
    let viewer_jwt =
        external_auth_token_for_role(&state, tenant.id, UserRole::Viewer, "cli-race-viewer").await;

    let (status, created) = request_as(
        app.clone(),
        Method::POST,
        &tickets_uri(tenant.id),
        ticket_body(CALLBACK),
        &viewer_jwt,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ticket = decode_json::<LoginTicketResponse>(created).ticket;

    let mut handles = Vec::new();
    for _ in 0..8 {
        let app = app.clone();
        let ticket = ticket.clone();
        handles.push(tokio::spawn(async move {
            request(
                app,
                Method::POST,
                EXCHANGE_URI,
                exchange_body(&ticket, CODE_VERIFIER),
            )
            .await
            .0
        }));
    }
    let mut ok_count = 0;
    let mut unauthorized_count = 0;
    for handle in handles {
        match handle.await.unwrap() {
            StatusCode::OK => ok_count += 1,
            StatusCode::UNAUTHORIZED => unauthorized_count += 1,
            other => panic!("unexpected concurrent exchange status: {other}"),
        }
    }
    assert_eq!(ok_count, 1);
    assert_eq!(unauthorized_count, 7);
}
