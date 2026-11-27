//! The Moonraker status WebSocket: identifies the client and answers object
//! subscriptions with the current object state, then forwards notifications.

use std::sync::Arc;

use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::Response,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{API_KEY, FakeState};

pub(super) async fn websocket(
    State(state): State<Arc<FakeState>>,
    upgrade: WebSocketUpgrade,
) -> Response {
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
