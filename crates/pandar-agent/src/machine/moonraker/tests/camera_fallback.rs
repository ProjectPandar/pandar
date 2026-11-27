use crate::machine::moonraker::{MoonrakerClient, camera::camera_client};

use super::fake_moonraker::{API_KEY, FakeMoonraker};

/// Webcam placement is probed one snapshot at a time:
/// 1. the API port serves the snapshot → the API client stays;
/// 2. the API port answers with an HTTP error → the nginx client wins;
/// 3. both answer with HTTP errors → the error names the nginx base that was tried;
/// 4. the API port cannot be connected to → that failure is propagated untouched.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moonraker_camera_falls_back_to_the_nginx_port() {
    let printer = FakeMoonraker::start().await;
    let api = MoonrakerClient::new(&printer.host, Some(API_KEY.to_owned()));
    let nginx = MoonrakerClient::new(&printer.nginx_host, None);
    let snapshot = "/webcam/snapshot.jpg";
    let served_by_api = format!("http://{}/webcam/snapshot.jpg", printer.nginx_host);
    assert_eq!(
        camera_client(&api, Some(nginx.clone()), &served_by_api)
            .await
            .unwrap(),
        api
    );
    assert_eq!(
        camera_client(&api, Some(nginx.clone()), snapshot)
            .await
            .unwrap(),
        nginx
    );

    let both_missing = camera_client(&api, Some(nginx.clone()), "/webcam/missing.jpg")
        .await
        .unwrap_err();
    assert!(
        format!("{both_missing:#}").contains(&format!("from {}", nginx.base_url())),
        "{both_missing:#}"
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let closed = listener.local_addr().unwrap().to_string();
    drop(listener);
    let unreachable = MoonrakerClient::new(&closed, None);
    let refused = camera_client(&unreachable, Some(nginx.clone()), snapshot)
        .await
        .unwrap_err();
    let refused = format!("{refused:#}");
    assert!(
        refused.contains("request Moonraker camera snapshot"),
        "{refused}"
    );
    assert!(!refused.contains(nginx.base_url()), "{refused}");
}
