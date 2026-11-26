use super::*;
use crate::repositories::{TenantTokenScope, UserRole};
use axum::http::header::{ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_ORIGIN, ORIGIN};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

const ARES_ORIGIN: &str = "https://ares.example";
const ALLOWED_ORIGINS: &str = "https://ares.example, http://127.0.0.1:5173";
const CALLBACK: &str = "https://ares.example/cloud-callback.html";
const CODE_VERIFIER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const CODE_CHALLENGE: &str = "ZtNPunH49FD35FWYhT5Tv8I7vRKQJ8uxMaL0_9eHjNA";

#[derive(Serialize)]
struct WebLoginTicketRequest<'a> {
    redirect_url: &'a str,
    code_challenge: &'a str,
}

#[derive(Serialize)]
struct WebTicketExchangeRequest<'a> {
    ticket: &'a str,
    code_verifier: &'a str,
}

#[derive(Serialize)]
struct TenantTokenCreateRequest<'a> {
    name: &'a str,
    scopes: &'a [&'a str],
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
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
struct PrinterEventTicketResponse {
    ticket: String,
}

fn ticket_body(redirect_url: &str) -> Option<Value> {
    Some(json_body(WebLoginTicketRequest {
        redirect_url,
        code_challenge: CODE_CHALLENGE,
    }))
}

fn exchange_body(ticket: &str, code_verifier: &str) -> Option<Value> {
    Some(json_body(WebTicketExchangeRequest {
        ticket,
        code_verifier,
    }))
}

fn tickets_uri(tenant_id: TenantId) -> String {
    format!("/api/v1/tenants/{tenant_id}/web/login-tickets")
}

const EXCHANGE_URI: &str = "/api/v1/web/login-tickets/exchange";

async fn web_state() -> AppState {
    external_auth_state(
        state()
            .await
            .with_cors_allowed_origins_for_tests(ALLOWED_ORIGINS),
    )
}

async fn operator(state: &AppState, tenant_id: TenantId, subject: &str) -> String {
    external_auth_token_for_role(state, tenant_id, UserRole::Operator, subject).await
}

async fn create_ticket(app: Router, tenant_id: TenantId, token: &str) -> String {
    let (status, body) = request_as(
        app,
        Method::POST,
        &tickets_uri(tenant_id),
        ticket_body(CALLBACK),
        token,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    decode_json::<LoginTicketResponse>(body).ticket
}

async fn web_session(app: Router, tenant_id: TenantId, token: &str) -> String {
    let ticket = create_ticket(app.clone(), tenant_id, token).await;
    let (status, body) = request(
        app,
        Method::POST,
        EXCHANGE_URI,
        exchange_body(&ticket, CODE_VERIFIER),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    decode_json::<ExchangeResponse>(body).token
}

#[tokio::test]
async fn web_login_ticket_exchange_issues_one_use_web_session_token() {
    let state = web_state().await;
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("web-exchange", "Web Exchange")
        .await
        .unwrap();
    let viewer =
        external_auth_token_for_role(&state, tenant.id, UserRole::Viewer, "web-viewer").await;
    let operator = operator(&state, tenant.id, "web-operator").await;

    let (status, _) = request_as(
        app.clone(),
        Method::POST,
        &tickets_uri(tenant.id),
        ticket_body(CALLBACK),
        &viewer,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, created) = request_as(
        app.clone(),
        Method::POST,
        &tickets_uri(tenant.id),
        ticket_body(CALLBACK),
        &operator,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let created = decode_json::<LoginTicketResponse>(created);
    assert!(created.ticket.starts_with("pandar_web_ticket_"));
    assert!(created.expires_at.ends_with('Z'));
    assert_eq!(created.redirect_url, CALLBACK);

    let (status, body) = request(
        app.clone(),
        Method::POST,
        "/api/v1/mobile/login-tickets/exchange",
        exchange_body(&created.ticket, CODE_VERIFIER),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        decode_json::<ErrorResponse>(body).error,
        "invalid_plugin_ticket"
    );

    let (status, body) = request(
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
    assert_eq!(
        decode_json::<ErrorResponse>(body).error,
        "invalid_plugin_ticket"
    );

    let (status, exchanged) = request(
        app.clone(),
        Method::POST,
        EXCHANGE_URI,
        exchange_body(&created.ticket, CODE_VERIFIER),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let exchanged = decode_json::<ExchangeResponse>(exchanged);
    assert!(exchanged.token.starts_with("pandar_web_"));
    assert!(exchanged.expires_at.ends_with('Z'));
    assert_eq!(exchanged.profile.user_name, "Ares web");
    assert_eq!(exchanged.profile.tenant_id, tenant.id.to_string());
    assert_eq!(exchanged.profile.tenant_name, "Web Exchange");

    let issued = state
        .auth()
        .list_tenant_tokens(tenant.id)
        .await
        .unwrap()
        .into_iter()
        .find(|token| token.name == "Ares web")
        .unwrap();
    assert_eq!(issued.scopes, [TenantTokenScope::WebSession]);
    assert_eq!(
        issued.created_by_user_id.as_deref(),
        Some(exchanged.profile.user_id.as_str())
    );

    let (status, body) = request(
        app,
        Method::POST,
        EXCHANGE_URI,
        exchange_body(&created.ticket, CODE_VERIFIER),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        decode_json::<ErrorResponse>(body).error,
        "invalid_plugin_ticket"
    );
}

#[tokio::test]
async fn web_login_ticket_requires_pkce_challenge() {
    let state = web_state().await;
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("web-pkce", "Web Pkce")
        .await
        .unwrap();
    let operator = operator(&state, tenant.id, "web-pkce-operator").await;

    for code_challenge in ["", "short", "not base64url ++++++++++++++++++++++++++++++"] {
        let (status, body) = request_as(
            app.clone(),
            Method::POST,
            &tickets_uri(tenant.id),
            Some(json_body(WebLoginTicketRequest {
                redirect_url: CALLBACK,
                code_challenge,
            })),
            &operator,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{code_challenge:?}");
        assert_eq!(
            decode_json::<ErrorResponse>(body).error,
            "invalid_code_challenge"
        );
    }
}

#[tokio::test]
async fn web_redirect_must_be_a_query_free_callback_on_an_allowlisted_origin() {
    let state = web_state().await;
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("web-redirect", "Web Redirect")
        .await
        .unwrap();
    let operator = operator(&state, tenant.id, "web-redirect-operator").await;

    for accepted in [
        CALLBACK,
        "https://ares.example/",
        "http://127.0.0.1:5173/cloud-callback.html",
    ] {
        let (status, body) = request_as(
            app.clone(),
            Method::POST,
            &tickets_uri(tenant.id),
            ticket_body(accepted),
            &operator,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{accepted} rejected: {body}");
    }

    for rejected in [
        "https://evil.example/cloud-callback.html",
        "http://ares.example/cloud-callback.html",
        "https://ares.example:8443/cloud-callback.html",
        "http://127.0.0.1:5174/cloud-callback.html",
        "http://localhost:5173/cloud-callback.html",
        "https://ares.example.evil.example/cloud-callback.html",
        "https://ares.example/cloud-callback.html?next=/",
        "https://ares.example/cloud-callback.html?",
        "https://ares.example/cloud-callback.html#fragment",
        "https://user@ares.example/cloud-callback.html",
        "https://user:pass@ares.example/cloud-callback.html",
        "ftp://ares.example/cloud-callback.html",
        "zip.iptables.pandar.android://auth/callback",
        "/cloud-callback.html",
        "not a url",
    ] {
        let (status, body) = request_as(
            app.clone(),
            Method::POST,
            &tickets_uri(tenant.id),
            ticket_body(rejected),
            &operator,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{rejected} accepted");
        assert_eq!(
            decode_json::<ErrorResponse>(body).error,
            "invalid_redirect_url"
        );
    }
}

#[tokio::test]
async fn web_redirect_is_rejected_when_no_origin_is_allowlisted() {
    let state = external_auth_state(state().await);
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("web-no-cors", "Web No Cors")
        .await
        .unwrap();
    let operator = operator(&state, tenant.id, "web-no-cors-operator").await;

    let (status, body) = request_as(
        app,
        Method::POST,
        &tickets_uri(tenant.id),
        ticket_body(CALLBACK),
        &operator,
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        decode_json::<ErrorResponse>(body).error,
        "invalid_redirect_url"
    );
}

#[tokio::test]
async fn web_exchange_answers_cors_for_the_allowlisted_origin() {
    let state = web_state().await;
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("web-cors", "Web Cors")
        .await
        .unwrap();
    let operator = operator(&state, tenant.id, "web-cors-operator").await;
    let ticket = create_ticket(app.clone(), tenant.id, &operator).await;

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(EXCHANGE_URI)
                .header(ORIGIN, ARES_ORIGIN)
                .header("content-type", "application/json")
                .body(Body::from(
                    exchange_body(&ticket, CODE_VERIFIER).unwrap().to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        headers
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .map(|value| value.to_str().unwrap()),
        Some(ARES_ORIGIN)
    );
    assert!(headers.get(ACCESS_CONTROL_ALLOW_CREDENTIALS).is_none());
}

#[tokio::test]
async fn web_session_acts_with_the_signed_in_users_current_role() {
    let state = web_state().await;
    let app = router(state.clone());
    let tenant = state
        .tenants()
        .create("web-role", "Web Role")
        .await
        .unwrap();
    let operator = operator(&state, tenant.id, "web-role-operator").await;
    let session = web_session(app.clone(), tenant.id, &operator).await;

    let (status, _) = request_as(
        app.clone(),
        Method::GET,
        &format!("/api/v1/tenants/{}/printers", tenant.id),
        None,
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let agent = state.agents().create(tenant.id, "agent").await.unwrap();
    let printer_id = insert_printer_fixture(state.database(), tenant.id, agent.id)
        .await
        .unwrap();
    let jobs_uri = format!("/api/v1/tenants/{}/printers/{printer_id}/jobs", tenant.id);
    let plate = || multipart_print_body(None, Some(("plate.3mf", "model/3mf", b"abc")), 1);
    let (status, body) =
        multipart_request_as(app.clone(), Method::POST, &jobs_uri, plate(), &session).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, _) = request_as(
        app.clone(),
        Method::POST,
        &format!("/api/v1/tenants/{}/tenant-tokens", tenant.id),
        Some(json_body(TenantTokenCreateRequest {
            name: "escalate",
            scopes: &[],
        })),
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let user = state
        .auth()
        .list_users_for_tenant(tenant.id)
        .await
        .unwrap()
        .into_iter()
        .find(|user| user.email == "web-role-operator@example.test")
        .unwrap();
    state
        .auth()
        .update_user_role(tenant.id, &user.id, UserRole::Viewer)
        .await
        .unwrap();
    let (status, body) =
        multipart_request_as(app, Method::POST, &jobs_uri, plate(), &session).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(decode_json::<ErrorResponse>(body).error, "role_forbidden");
}

#[tokio::test]
async fn web_session_opens_printer_events_websocket_from_allowlisted_origin() {
    let state = web_state().await;
    let app = router(state.clone());
    let tenant = state.tenants().create("web-ws", "Web Ws").await.unwrap();
    let operator = operator(&state, tenant.id, "web-ws-operator").await;
    let session = web_session(app.clone(), tenant.id, &operator).await;

    let (status, body) = request_as(
        app.clone(),
        Method::POST,
        &format!("/api/v1/tenants/{}/printer-events/tickets", tenant.id),
        None,
        &session,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let ticket = decode_json::<PrinterEventTicketResponse>(body).ticket;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut ws_request = format!(
        "ws://{addr}/api/v1/tenants/{}/printer-events?ticket={ticket}",
        tenant.id
    )
    .into_client_request()
    .unwrap();
    ws_request
        .headers_mut()
        .insert(ORIGIN, ARES_ORIGIN.parse().unwrap());

    let (ws, _) = tokio_tungstenite::connect_async(ws_request).await.unwrap();
    drop(ws);
}
