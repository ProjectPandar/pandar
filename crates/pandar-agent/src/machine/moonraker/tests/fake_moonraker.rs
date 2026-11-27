//! In-process Moonraker double serving the HTTP and WebSocket surface a
//! Snapmaker U1 exposes. Klipper object payloads are open-ended JSON, so the
//! double stores them as `serde_json::Value`. Two listeners model the real
//! layout: the Moonraker API port, and an nginx port that serves the webcam
//! the webcam list points at.

mod objects;
mod websocket;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Multipart, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use objects::initial_objects;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, broadcast};
use websocket::websocket;

pub(super) const API_KEY: &str = "u1-moonraker-secret";
pub(super) const CPU_SERIAL: &str = "a1b2c3d4e5f60718";
pub(super) const JPEG_FRAME: &[u8] = &[0xff, 0xd8, 0x55, 0x31, 0xff, 0xd9];
/// Described commands the U1 firmware exposes; `FILAMENT_DT_UPDATE` is registered
/// without a description and is detected through the `filament_detect` object.
const FILAMENT_COMMANDS: &[&str] = &["AUTO_FEEDING"];

pub(super) struct FakeMoonraker {
    /// Moonraker API, like `7125` on a U1.
    pub(super) host: String,
    /// The nginx proxy that serves the webcam, like port `80` on a U1.
    pub(super) nginx_host: String,
    state: Arc<FakeState>,
}

struct FakeState {
    objects: Mutex<Map<String, Value>>,
    requests: Mutex<Vec<String>>,
    webcam_requests: Mutex<Vec<String>>,
    gcode_help: Vec<&'static str>,
    identified_with_api_key: Mutex<Option<bool>>,
    notifications: broadcast::Sender<String>,
    nginx_host: Mutex<String>,
}

impl FakeMoonraker {
    /// A U1 with the `print_task_config` filament extensions.
    pub(super) async fn start() -> Self {
        Self::start_with_gcode_help(FILAMENT_COMMANDS).await
    }

    /// A printer whose firmware registers only the given described G-code
    /// commands; without `AUTO_FEEDING` the macro section is absent too.
    pub(super) async fn start_with_gcode_help(gcode_help: &[&'static str]) -> Self {
        let (notifications, _) = broadcast::channel(64);
        let mut objects = initial_objects();
        if !gcode_help.contains(&"AUTO_FEEDING") {
            objects.remove("gcode_macro AUTO_FEEDING");
        }
        let state = Arc::new(FakeState {
            objects: Mutex::new(objects),
            requests: Mutex::new(Vec::new()),
            webcam_requests: Mutex::new(Vec::new()),
            gcode_help: gcode_help.to_vec(),
            identified_with_api_key: Mutex::new(None),
            notifications,
            nginx_host: Mutex::new(String::new()),
        });
        let nginx = Router::new()
            .route("/webcam/snapshot.jpg", get(snapshot))
            .route("/webcam/webrtc", get(webrtc))
            .route("/webcam/stream.mjpg", get(stream))
            .with_state(Arc::clone(&state));
        let nginx_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let nginx_addr: SocketAddr = nginx_listener.local_addr().unwrap();
        *state.nginx_host.lock().await = nginx_addr.to_string();
        tokio::spawn(async move { axum::serve(nginx_listener, nginx).await.unwrap() });

        let app = Router::new()
            .route("/printer/info", get(printer_info))
            .route("/machine/system_info", get(system_info))
            .route("/printer/objects/list", get(object_list))
            .route("/printer/objects/query", post(object_query))
            .route("/printer/gcode/help", get(printer_gcode_help))
            .route("/server/webcams/list", get(webcams))
            .route("/server/files/metadata", post(file_metadata))
            .route("/server/files/upload", post(upload))
            .route("/printer/print/start", post(print_start))
            .route("/printer/print/pause", post(print_pause))
            .route("/printer/print/resume", post(print_resume))
            .route("/printer/print/cancel", post(print_cancel))
            .route("/printer/gcode/script", post(gcode_script))
            .route("/websocket", get(websocket))
            .layer(middleware::from_fn(require_api_key))
            .with_state(Arc::clone(&state));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address: SocketAddr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self {
            host: address.to_string(),
            nginx_host: nginx_addr.to_string(),
            state,
        }
    }

    /// Requests that change printer state, in arrival order.
    pub(super) async fn requests(&self) -> Vec<String> {
        self.state.requests.lock().await.clone()
    }

    /// Webcam resources fetched, in arrival order.
    pub(super) async fn webcam_requests(&self) -> Vec<String> {
        self.state.webcam_requests.lock().await.clone()
    }

    pub(super) async fn identified_with_api_key(&self) -> Option<bool> {
        *self.state.identified_with_api_key.lock().await
    }
}

async fn require_api_key(request: Request, next: Next) -> Response {
    let authorized = request
        .headers()
        .get("X-Api-Key")
        .is_some_and(|value| value.as_bytes() == API_KEY.as_bytes());
    if !authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"code": 401, "message": "Unauthorized"}})),
        )
            .into_response();
    }
    next.run(request).await
}

fn result(value: Value) -> Json<Value> {
    Json(json!({ "result": value }))
}

async fn printer_info() -> Json<Value> {
    result(json!({"state": "ready", "state_message": "Printer is ready", "hostname": "lava"}))
}

async fn system_info() -> Json<Value> {
    result(json!({"system_info": {
        "cpu_info": {"serial_number": CPU_SERIAL},
        "network": {"wlan0": {"mac_address": "02:00:00:aa:bb:cc"}},
        "product_info": {"machine_type": "Snapmaker U1", "device_name": "Index U1"}
    }}))
}

async fn object_list(State(state): State<Arc<FakeState>>) -> Json<Value> {
    let objects: Vec<String> = state.objects.lock().await.keys().cloned().collect();
    result(json!({ "objects": objects }))
}

#[derive(Deserialize)]
struct ObjectQuery {
    objects: Map<String, Value>,
}

async fn object_query(
    State(state): State<Arc<FakeState>>,
    Json(query): Json<ObjectQuery>,
) -> Json<Value> {
    result(json!({"eventtime": 1.0, "status": state.select(query.objects.keys()).await}))
}

async fn printer_gcode_help(State(state): State<Arc<FakeState>>) -> Json<Value> {
    let help: Map<String, Value> = state
        .gcode_help
        .iter()
        .map(|command| ((*command).to_owned(), Value::String(String::new())))
        .collect();
    result(Value::Object(help))
}

async fn webcams(State(state): State<Arc<FakeState>>) -> Json<Value> {
    let nginx_host = state.nginx_host.lock().await.clone();
    result(json!({"webcams": [{
        "name": "case", "enabled": true, "service": "webrtc-camerastreamer",
        "stream_url": format!("http://{nginx_host}/webcam/webrtc"),
        "snapshot_url": format!("http://{nginx_host}/webcam/snapshot.jpg")
    }]}))
}

#[derive(Deserialize)]
struct MetadataQuery {
    filename: String,
}

async fn file_metadata(
    State(state): State<Arc<FakeState>>,
    Json(query): Json<MetadataQuery>,
) -> Response {
    if !query.filename.starts_with("pandar/") {
        return (StatusCode::NOT_FOUND, "file not found").into_response();
    }
    let layers = state.objects.lock().await["print_stats"]["info"]["total_layer"]
        .as_u64()
        .unwrap_or(0);
    result(json!({
        "filename": query.filename, "estimated_time": 4800.0, "layer_count": layers
    }))
    .into_response()
}

async fn snapshot(State(state): State<Arc<FakeState>>) -> impl IntoResponse {
    state.record_webcam("snapshot").await;
    ([(header::CONTENT_TYPE, "image/jpeg")], JPEG_FRAME)
}

/// The WebRTC page the U1 advertises as its stream URL; not an MJPEG stream.
async fn webrtc(State(state): State<Arc<FakeState>>) -> impl IntoResponse {
    state.record_webcam("webrtc").await;
    ([(header::CONTENT_TYPE, "text/html")], "<html>webrtc</html>")
}

/// Three multipart frames, then the stream ends like a camera restarting.
async fn stream(State(state): State<Arc<FakeState>>) -> Response {
    state.record_webcam("stream").await;
    let mut body = Vec::new();
    for _ in 0..3 {
        body.extend_from_slice(b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: ");
        body.extend_from_slice(JPEG_FRAME.len().to_string().as_bytes());
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(JPEG_FRAME);
        body.extend_from_slice(b"\r\n");
    }
    (
        [(
            header::CONTENT_TYPE,
            "multipart/x-mixed-replace; boundary=frame",
        )],
        Body::from(body),
    )
        .into_response()
}

async fn upload(State(state): State<Arc<FakeState>>, mut multipart: Multipart) -> Json<Value> {
    let (mut root, mut path, mut filename, mut size) =
        (String::new(), String::new(), String::new(), 0);
    while let Some(field) = multipart.next_field().await.unwrap() {
        match field.name().unwrap() {
            "root" => root = field.text().await.unwrap(),
            "path" => path = field.text().await.unwrap(),
            "file" => {
                filename = field.file_name().unwrap().to_owned();
                size = field.bytes().await.unwrap().len();
            }
            other => panic!("unexpected upload field {other}"),
        }
    }
    let item = format!("{path}/{filename}");
    state
        .record(format!("upload root={root} path={item} bytes={size}"))
        .await;
    result(json!({"item": {"path": item, "root": root}, "action": "create_file"}))
}

#[derive(Deserialize)]
struct StartPrint {
    filename: String,
}

async fn print_start(
    State(state): State<Arc<FakeState>>,
    Json(start): Json<StartPrint>,
) -> Json<Value> {
    state
        .record(format!("print_start {}", start.filename))
        .await;
    state
        .update(json!({
            "print_stats": {
                "state": "printing", "filename": start.filename, "print_duration": 600.0,
                "info": {"current_layer": 3, "total_layer": 120}
            },
            "virtual_sdcard": {"progress": 0.25},
            "display_status": {"progress": 0.25}
        }))
        .await;
    result(json!("ok"))
}

async fn print_pause(State(state): State<Arc<FakeState>>) -> Json<Value> {
    state.print_action("pause", "paused").await
}

async fn print_resume(State(state): State<Arc<FakeState>>) -> Json<Value> {
    state.print_action("resume", "printing").await
}

async fn print_cancel(State(state): State<Arc<FakeState>>) -> Json<Value> {
    state.print_action("cancel", "cancelled").await
}

#[derive(Deserialize)]
struct GcodeScript {
    script: String,
}

async fn gcode_script(
    State(state): State<Arc<FakeState>>,
    Json(gcode): Json<GcodeScript>,
) -> Json<Value> {
    state.record(format!("gcode {}", gcode.script)).await;
    let script = gcode.script.as_str();
    if let Some(dwell) = script.strip_prefix("G4 P") {
        // Klipper answers only when the script finishes; a dwell stands in for
        // a long macro such as filament feeding.
        let millis: u64 = dwell.trim().parse().unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
        state.record(format!("gcode-done {script}")).await;
    }
    if script.starts_with("SET_LED LED=cavity_led RED=1") {
        state
            .update(json!({"led cavity_led": {"color_data": [[1.0, 1.0, 1.0, 1.0]]}}))
            .await;
    } else if let Some(rest) = script.strip_prefix("SET_FAN_SPEED FAN=") {
        let (fan, speed) = rest.split_once(" SPEED=").unwrap();
        let object = format!("fan_generic {fan}");
        state
            .update(json!({object: {"speed": speed.parse::<f64>().unwrap()}}))
            .await;
    } else if let Some(rest) = script.strip_prefix("M106 S") {
        state
            .update(json!({"fan": {"speed": rest.parse::<f64>().unwrap() / 255.0}}))
            .await;
    } else if let Some(rest) = script.strip_prefix("SET_HEATER_TEMPERATURE HEATER=") {
        let (heater, target) = rest.split_once(" TARGET=").unwrap();
        state
            .update(json!({heater: {"target": target.parse::<f64>().unwrap()}}))
            .await;
    } else if let Some(tool) = script.strip_prefix('T')
        && let Ok(tool) = tool.parse::<u32>()
    {
        let extruder = if tool == 0 {
            "extruder".to_owned()
        } else {
            format!("extruder{tool}")
        };
        state
            .update(json!({"toolhead": {"extruder": extruder}}))
            .await;
    }
    result(json!("ok"))
}

impl FakeState {
    async fn record(&self, request: String) {
        self.requests.lock().await.push(request);
    }

    async fn record_webcam(&self, resource: &str) {
        self.webcam_requests.lock().await.push(resource.to_owned());
    }

    async fn select<'a>(&self, names: impl Iterator<Item = &'a String>) -> Map<String, Value> {
        let objects = self.objects.lock().await;
        names
            .filter_map(|name| Some((name.clone(), objects.get(name)?.clone())))
            .collect()
    }

    /// Applies a shallow per-object update and pushes it as Moonraker would.
    async fn update(&self, update: Value) {
        let mut objects = self.objects.lock().await;
        for (name, fields) in update.as_object().unwrap() {
            let object = objects.get_mut(name).unwrap().as_object_mut().unwrap();
            for (field, value) in fields.as_object().unwrap() {
                object.insert(field.clone(), value.clone());
            }
        }
        drop(objects);
        self.notify(&update);
    }

    fn notify(&self, update: &Value) {
        let notification = json!({
            "jsonrpc": "2.0", "method": "notify_status_update", "params": [update, 2.0]
        });
        let _ = self.notifications.send(notification.to_string());
    }

    async fn print_action(&self, action: &str, print_state: &str) -> Json<Value> {
        self.record(format!("print_{action}")).await;
        self.update(json!({"print_stats": {"state": print_state}}))
            .await;
        result(json!("ok"))
    }
}
