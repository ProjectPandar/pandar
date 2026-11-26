use super::*;
use axum::http::{
    HeaderMap,
    header::{
        ACCESS_CONTROL_ALLOW_CREDENTIALS, ACCESS_CONTROL_ALLOW_HEADERS,
        ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_REQUEST_HEADERS,
        ACCESS_CONTROL_REQUEST_METHOD, ORIGIN,
    },
};

const ARES_ORIGIN: &str = "https://ares.example";
const ALLOWED_ORIGINS: &str = "https://ares.example, http://127.0.0.1:5173";

async fn preflight(app: Router, uri: &str, origin: &str) -> (StatusCode, HeaderMap) {
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri(uri)
                .header(ORIGIN, origin)
                .header(ACCESS_CONTROL_REQUEST_METHOD, "POST")
                .header(ACCESS_CONTROL_REQUEST_HEADERS, "authorization,content-type")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    (response.status(), response.headers().clone())
}

fn header<'a>(headers: &'a HeaderMap, name: &axum::http::HeaderName) -> Option<&'a str> {
    headers.get(name).map(|value| value.to_str().unwrap())
}

fn job_upload_uri() -> String {
    format!(
        "/api/v1/tenants/{}/printers/{}/jobs",
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4()
    )
}

#[tokio::test]
async fn preflight_from_allowed_origin_is_answered_without_auth() {
    let app = router(
        state()
            .await
            .with_cors_allowed_origins_for_tests(ALLOWED_ORIGINS),
    );

    let (status, headers) = preflight(app, &job_upload_uri(), ARES_ORIGIN).await;

    assert!(status.is_success(), "preflight status {status}");
    assert_eq!(
        header(&headers, &ACCESS_CONTROL_ALLOW_ORIGIN),
        Some(ARES_ORIGIN)
    );
    let methods = header(&headers, &ACCESS_CONTROL_ALLOW_METHODS).unwrap();
    assert!(methods.contains("POST"), "allowed methods {methods}");
    let allowed_headers = header(&headers, &ACCESS_CONTROL_ALLOW_HEADERS)
        .unwrap()
        .to_ascii_lowercase();
    assert!(allowed_headers.contains("authorization"));
    assert!(allowed_headers.contains("content-type"));
    assert_eq!(header(&headers, &ACCESS_CONTROL_ALLOW_CREDENTIALS), None);
}

#[tokio::test]
async fn preflight_from_unlisted_origin_is_not_allowed() {
    let app = router(
        state()
            .await
            .with_cors_allowed_origins_for_tests(ALLOWED_ORIGINS),
    );

    let (_, headers) = preflight(app, &job_upload_uri(), "https://evil.example").await;

    assert_eq!(header(&headers, &ACCESS_CONTROL_ALLOW_ORIGIN), None);
    assert_eq!(header(&headers, &ACCESS_CONTROL_ALLOW_CREDENTIALS), None);
}

#[tokio::test]
async fn unset_allowlist_sends_no_cors_headers() {
    let app = router(state().await);

    let (_, headers) = preflight(app, &job_upload_uri(), ARES_ORIGIN).await;

    assert!(
        headers
            .keys()
            .all(|name| !name.as_str().starts_with("access-control-")),
        "unexpected CORS headers {headers:?}"
    );
}

#[tokio::test]
async fn routes_outside_client_api_do_not_answer_cors() {
    let app = router(
        state()
            .await
            .with_cors_allowed_origins_for_tests(ALLOWED_ORIGINS),
    );

    for uri in [
        "/api/v1/plugin/printers",
        "/api/v1/bootstrap/tenant-admin",
        "/api/v1/summary",
    ] {
        let (_, headers) = preflight(app.clone(), uri, ARES_ORIGIN).await;
        assert_eq!(
            header(&headers, &ACCESS_CONTROL_ALLOW_ORIGIN),
            None,
            "{uri} answered CORS"
        );
    }
}

#[tokio::test]
async fn bearer_request_from_allowed_origin_exposes_response_to_origin() {
    let state = state()
        .await
        .with_cors_allowed_origins_for_tests(ALLOWED_ORIGINS);
    let app = router(state.clone());
    let tenant = state.tenants().create("cors", "Cors").await.unwrap();
    let tenant_id = tenant.id.to_string();
    let token = read_only_tenant_token(&state, &tenant_id, "ares").await;

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/v1/tenants/{tenant_id}/printers"))
                .header(ORIGIN, "http://127.0.0.1:5173")
                .header(AUTHORIZATION, format!("Bearer {token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        header(headers, &ACCESS_CONTROL_ALLOW_ORIGIN),
        Some("http://127.0.0.1:5173")
    );
    assert_eq!(header(headers, &ACCESS_CONTROL_ALLOW_CREDENTIALS), None);
}

#[test]
fn allowlist_rejects_entries_that_are_not_exact_origins() {
    for value in [
        "*",
        "null",
        "https://*.example.com",
        "https://ares.example/",
        "https://ares.example/app",
        "https://ares.example?x=1",
        "https://user@ares.example",
        "ares.example",
        "ftp://ares.example",
    ] {
        assert!(
            crate::config::parse_cors_allowed_origins(value).is_err(),
            "{value:?} was accepted"
        );
    }
    assert_eq!(
        crate::config::parse_cors_allowed_origins(" , ")
            .unwrap()
            .len(),
        0
    );
}
