use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use super::super::*;
use super::support::*;

#[tokio::test]
async fn plugin_firmware_typed_json_validation_and_shared_body_limit_precede_dispatch() {
    let mut fixture = FirmwareRouteFixture::new("plugin-firmware-validation").await;
    for (suffix, body) in [
        ("/refresh", "{".to_owned()),
        ("/refresh", json!({"sequence_id": 7}).to_string()),
        (
            "/prepare",
            json!({"command":"unknown","sequence_id":"1","src_id":1}).to_string(),
        ),
        (
            "/prepare",
            json!({"command":"upgrade_confirm","sequence_id":"1","src_id":"1"}).to_string(),
        ),
        (
            "/prepare",
            json!({"command":"start","sequence_id":"1","src_id":1,"module":7,"version":"v"}).to_string(),
        ),
        (
            "/prepare",
            json!({"command":"start","sequence_id":"1","src_id":1,"module":"ota","version":false}).to_string(),
        ),
        (
            "/prepare",
            json!({"command":"mc_for_ams_firmware_upgrade","sequence_id":"1","src_id":1,"id":2147483648_i64}).to_string(),
        ),
        (
            "/execute",
            json!({"prepared_token":7,"command":upgrade_command("1")}).to_string(),
        ),
        (
            "/execute",
            json!({"prepared_token":"token","command":{"command":"start"}}).to_string(),
        ),
    ] {
        let (status, response) = raw_json_status(
            fixture.app(),
            Method::POST,
            &fixture.uri(suffix),
            Some(&fixture.auth),
            body,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{suffix}: {response}");
        if suffix == "/execute" {
            assert_eq!(
                serde_json::from_str::<Value>(&response).unwrap()["phase"],
                "pre_publish_failure"
            );
        }
    }

    for (suffix, body) in [
        ("/refresh", json!({"sequence_id":""})),
        (
            "/prepare",
            json!({"command":"upgrade_confirm","sequence_id":"","src_id":1}),
        ),
        ("/prepare", start_metadata("1", "", "v")),
        ("/prepare", start_metadata("1", "ota", "")),
        (
            "/execute",
            json!({"prepared_token":"","command":upgrade_command("1")}),
        ),
        (
            "/execute",
            json!({"prepared_token":"token","command":start_command("1", "", "ota", "v")}),
        ),
    ] {
        let (status, response) = request_as(
            fixture.app(),
            Method::POST,
            &fixture.uri(suffix),
            Some(body),
            &fixture.auth,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{suffix}");
        if suffix == "/execute" {
            assert_eq!(response["phase"], "pre_publish_failure");
        }
    }

    let huge = "x".repeat(64 * 1024);
    for (suffix, body) in [
        ("/refresh", json!({"sequence_id": &huge})),
        ("/prepare", start_metadata("1", &huge, "v")),
        ("/prepare", start_metadata("1", "ota", &huge)),
        (
            "/execute",
            json!({"prepared_token":"token","command":start_command("1", &huge, "ota", "v")}),
        ),
    ] {
        let (status, response) = raw_json_status(
            fixture.app(),
            Method::POST,
            &fixture.uri(suffix),
            Some(&fixture.auth),
            body.to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{suffix}");
        assert!(!response.contains(URL_SENTINEL));
        if suffix == "/execute" {
            assert_eq!(
                serde_json::from_str::<Value>(&response).unwrap()["phase"],
                "pre_publish_failure"
            );
        }
    }
    assert!(fixture.commands.try_recv().is_err());
    assert_eq!(fixture.state.commands().count().await.unwrap(), 0);
}

#[tokio::test]
async fn firmware_execute_accepts_command_derived_from_exact_limit_studio_start() {
    let mut fixture = FirmwareRouteFixture::new("plugin-firmware-studio-body-boundary").await;
    let sequence_id = "studio-body-boundary";
    let (prepared, _) = fixture
        .prepare(start_metadata(sequence_id, "ota", "v"))
        .await;
    let prepared_token = prepared["prepared_token"].as_str().unwrap();

    let studio_body = exact_body(
        65_536,
        |url| json!({"upgrade": start_command(sequence_id, url, "ota", "v")}),
    );
    let studio_envelope = serde_json::from_str::<Value>(&studio_body).unwrap();
    let execute_body = json!({
        "prepared_token": prepared_token,
        "command": studio_envelope["upgrade"].clone()
    })
    .to_string();
    assert_eq!(execute_body.len(), 65_592);

    let request = spawn_raw(&fixture, "/execute", execute_body);
    let outbound = fixture.next_command().await;
    fixture
        .generic_failure(command_id(&outbound), "known before publish")
        .await;
    let (status, response) = request.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{response}");
    assert_eq!(
        serde_json::from_str::<Value>(&response).unwrap()["phase"],
        "pre_publish_failure"
    );
}

fn exact_body(target: usize, build: impl Fn(&str) -> Value) -> String {
    let overhead = build("").to_string().len();
    let body = build(&"x".repeat(target - overhead)).to_string();
    assert_eq!(body.len(), target);
    body
}

fn spawn_raw(
    fixture: &FirmwareRouteFixture,
    suffix: &str,
    body: String,
) -> tokio::task::JoinHandle<(StatusCode, String)> {
    let app = fixture.app();
    let uri = fixture.uri(suffix);
    let auth = fixture.auth.clone();
    tokio::spawn(async move { raw_json_status(app, Method::POST, &uri, Some(&auth), body).await })
}
