//! Moonraker's status WebSocket: the JSON-RPC identify/subscribe handshake and
//! the message reader shared by report forwarding and diagnostics.

use std::time::Duration;

use anyhow::{Context, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tokio::{net::TcpStream, time::timeout};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{Message, client::IntoClientRequest, http::HeaderValue},
};

use super::{MoonrakerClient, client::KlipperObjects, status::ObjectSelection};

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const IDENTIFY_ID: u64 = 1;

pub(super) type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub(super) async fn connect(client: &MoonrakerClient) -> anyhow::Result<Socket> {
    let mut request = client
        .websocket_url()
        .into_client_request()
        .context("build Moonraker WebSocket request")?;
    if let Some(key) = client.api_key() {
        request.headers_mut().insert(
            "X-Api-Key",
            HeaderValue::from_str(key).context("encode Moonraker API key header")?,
        );
    }
    let (socket, _) = connect_async(request)
        .await
        .context("connect Moonraker WebSocket")?;
    Ok(socket)
}

/// Completes the identify and subscribe handshake on a fresh status socket
/// and waits for Moonraker to accept the subscription.
pub(super) async fn probe_websocket(client: &MoonrakerClient, version: &str) -> anyhow::Result<()> {
    let mut socket = connect(client).await?;
    identify(&mut socket, client, version).await?;
    let mut next_id = IDENTIFY_ID + 1;
    let subscribe_id = subscribe(&mut socket, &ObjectSelection::default(), &mut next_id).await?;
    loop {
        let message = timeout(PROBE_TIMEOUT, read_rpc(&mut socket))
            .await
            .context("Moonraker did not answer the subscription in time")??;
        if let Some(error) = message.error {
            bail!(
                "Moonraker JSON-RPC request {:?} failed: {}",
                message.id,
                error.message
            );
        }
        if message.id == Some(subscribe_id) {
            return Ok(());
        }
    }
}

pub(super) async fn identify(
    socket: &mut Socket,
    client: &MoonrakerClient,
    version: &str,
) -> anyhow::Result<()> {
    send_request(
        socket,
        IDENTIFY_ID,
        "server.connection.identify",
        IdentifyParams {
            client_name: "pandar-agent",
            version,
            client_type: "other",
            url: "https://github.com/ProjectPandar/pandar",
            api_key: client.api_key(),
        },
    )
    .await
}

/// Reads the next JSON-RPC message, skipping non-text frames.
pub(super) async fn read_rpc(socket: &mut Socket) -> anyhow::Result<RpcMessage> {
    loop {
        let message = socket
            .next()
            .await
            .context("Moonraker WebSocket closed")?
            .context("read Moonraker WebSocket message")?;
        let Message::Text(text) = message else {
            continue;
        };
        return serde_json::from_str(&text).context("decode Moonraker JSON-RPC message");
    }
}

pub(super) async fn subscribe(
    socket: &mut Socket,
    selection: &ObjectSelection,
    next_id: &mut u64,
) -> anyhow::Result<u64> {
    let id = *next_id;
    *next_id += 1;
    send_request(
        socket,
        id,
        "printer.objects.subscribe",
        SubscribeParams {
            objects: selection
                .subscribed
                .iter()
                .map(|name| (name.as_str(), None))
                .collect(),
        },
    )
    .await?;
    Ok(id)
}

async fn send_request<P: Serialize>(
    socket: &mut Socket,
    id: u64,
    method: &str,
    params: P,
) -> anyhow::Result<()> {
    let request = serde_json::to_string(&RpcRequest {
        jsonrpc: "2.0",
        method,
        params,
        id,
    })
    .expect("Moonraker JSON-RPC request is serializable");
    socket
        .send(Message::text(request))
        .await
        .with_context(|| format!("send Moonraker {method} request"))
}

pub(super) fn decode<T: for<'de> Deserialize<'de>>(
    raw: Option<&RawValue>,
    what: &str,
) -> anyhow::Result<T> {
    let raw = raw.with_context(|| format!("Moonraker {what} is missing"))?;
    serde_json::from_str(raw.get()).with_context(|| format!("decode Moonraker {what}"))
}

#[derive(Serialize)]
struct RpcRequest<'a, P> {
    jsonrpc: &'static str,
    method: &'a str,
    params: P,
    id: u64,
}

#[derive(Serialize)]
struct IdentifyParams<'a> {
    client_name: &'static str,
    version: &'a str,
    #[serde(rename = "type")]
    client_type: &'static str,
    url: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<&'a str>,
}

#[derive(Serialize)]
struct SubscribeParams<'a> {
    objects: std::collections::BTreeMap<&'a str, Option<()>>,
}

#[derive(Deserialize)]
pub(super) struct RpcMessage {
    pub(super) id: Option<u64>,
    pub(super) method: Option<String>,
    pub(super) params: Option<Box<RawValue>>,
    pub(super) result: Option<Box<RawValue>>,
    pub(super) error: Option<RpcError>,
}

#[derive(Deserialize)]
pub(super) struct RpcError {
    pub(super) message: String,
}

#[derive(Deserialize)]
pub(super) struct SubscribeResult {
    pub(super) status: KlipperObjects,
}
