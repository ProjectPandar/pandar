//! In-process Moonraker double serving the HTTP and WebSocket surface a
//! Snapmaker U1 exposes. Klipper object payloads are open-ended JSON, so the
//! double stores them as `serde_json::Value`.

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Json, Router,
    body::Bytes,
    extract::{
        Multipart, Request, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::{Mutex, broadcast};

pub(super) const API_KEY: &str = "u1-moonraker-secret";
pub(super) const CPU_SERIAL: &str = "a1b2c3d4e5f60718";
pub(super) const JPEG_FRAME: &[u8] = &[0xff, 0xd8, 0x55, 0x31, 0xff, 0xd9];

pub(super) struct FakeMoonraker {
    pub(super) host: String,
    state: Arc<FakeState>,
}

struct FakeState {
    objects: Mutex<Map<String, Value>>,
    requests: Mutex<Vec<String>>,
    identified_with_api_key: Mutex<Option<bool>>,
    notifications: broadcast::Sender<String>,
}

impl FakeMoonraker {
    pub(super) async fn start() -> Self {
        let (notifications, _) = broadcast::channel(64);
        let state = Arc::new(FakeState {
            objects: Mutex::new(initial_objects()),
            requests: Mutex::new(Vec::new()),
            identified_with_api_key: Mutex::new(None),
            notifications,
        });
        let app = Router::new()
            .route("/printer/info", get(printer_info))
            .route("/machine/system_info", get(system_info))
            .route("/printer/objects/list", get(object_list))
            .route("/printer/objects/query", post(object_query))
            .route("/server/webcams/list", get(webcams))
            .route("/webcam/", get(snapshot))
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
            state,
        }
    }

    /// Requests that change printer state, in arrival order.
    pub(super) async fn requests(&self) -> Vec<String> {
        self.state.requests.lock().await.clone()
    }

    pub(super) async fn identified_with_api_key(&self) -> Option<bool> {
        *self.state.identified_with_api_key.lock().await
    }
}

fn initial_objects() -> Map<String, Value> {
    let Value::Object(objects) = json!({
        "webhooks": {"state": "ready", "state_message": "Printer is ready"},
        "print_stats": {
            "state": "standby", "filename": "", "print_duration": 0.0, "message": "",
            "info": {"current_layer": null, "total_layer": null}
        },
        "virtual_sdcard": {"progress": 0.0},
        "display_status": {"progress": 0.0},
        "toolhead": {"extruder": "extruder"},
        "gcode_move": {"speed_factor": 1.0},
        "heater_bed": {"temperature": 24.5, "target": 0.0},
        "fan": {"speed": 0.0},
        "extruder": {"temperature": 25.0, "target": 0.0},
        "extruder1": {"temperature": 25.5, "target": 0.0},
        "extruder2": {"temperature": 26.0, "target": 0.0},
        "extruder3": {"temperature": 26.5, "target": 0.0},
        "led cavity_led": {"color_data": [[0.0, 0.0, 0.0, 0.0]]},
        "filament_detect": {"info": []},
        "print_task_config": {
            "filament_exist": [true, true, false, false],
            "filament_vendor": ["Snapmaker", "Generic", "", ""],
            "filament_type": ["PLA", "PETG", "", ""],
            "filament_sub_type": ["Basic", "", "", ""],
            "filament_color_rgba": ["ff0000ff", "00ff00ff", "", ""]
        }
    }) else {
        unreachable!("initial objects are a JSON object")
    };
    objects
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
        "network": {"wlan0": {"mac_address": "02:00:00:aa:bb:cc"}}
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

async fn webcams() -> Json<Value> {
    result(json!({"webcams": [{
        "name": "U1 Camera", "enabled": true, "service": "mjpegstreamer",
        "stream_url": "/webcam/?action=stream", "snapshot_url": "/webcam/?action=snapshot"
    }]}))
}

async fn snapshot() -> Bytes {
    Bytes::from_static(JPEG_FRAME)
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
    if gcode.script.starts_with("SET_LED LED=cavity_led RED=1") {
        state
            .update(json!({"led cavity_led": {"color_data": [[1.0, 1.0, 1.0, 1.0]]}}))
            .await;
    }
    result(json!("ok"))
}

async fn websocket(State(state): State<Arc<FakeState>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| serve_websocket(state, socket))
}

#[derive(Deserialize)]
struct RpcRequest {
    method: String,
    #[serde(default)]
    params: Map<String, Value>,
    id: u64,
}

async fn serve_websocket(state: Arc<FakeState>, mut socket: WebSocket) {
    let mut notifications = state.notifications.subscribe();
    loop {
        tokio::select! {
            message = socket.recv() => {
                let Some(Ok(Message::Text(text))) = message else { return };
                let request: RpcRequest = serde_json::from_str(&text).unwrap();
                let response = match request.method.as_str() {
                    "server.connection.identify" => {
                        let api_key = request.params.get("api_key").and_then(Value::as_str);
                        *state.identified_with_api_key.lock().await = Some(api_key == Some(API_KEY));
                        json!({"connection_id": 7})
                    }
                    "printer.objects.subscribe" => {
                        let requested = request.params["objects"].as_object().unwrap().clone();
                        json!({"eventtime": 1.0, "status": state.select(requested.keys()).await})
                    }
                    other => panic!("unexpected Moonraker RPC {other}"),
                };
                let reply = json!({"jsonrpc": "2.0", "result": response, "id": request.id});
                if socket.send(Message::text(reply.to_string())).await.is_err() {
                    return;
                }
            }
            notification = notifications.recv() => {
                let Ok(notification) = notification else { return };
                if socket.send(Message::text(notification)).await.is_err() {
                    return;
                }
            }
        }
    }
}

impl FakeState {
    async fn record(&self, request: String) {
        self.requests.lock().await.push(request);
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
