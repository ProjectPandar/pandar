use std::{collections::BTreeMap, time::Duration};

use anyhow::{Context, bail};
use reqwest::{Method, RequestBuilder, multipart};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};

const API_KEY_HEADER: &str = "X-Api-Key";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Thin typed client for the Moonraker HTTP API.
#[derive(Debug, Clone)]
pub struct MoonrakerClient {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl PartialEq for MoonrakerClient {
    fn eq(&self, other: &Self) -> bool {
        self.base_url == other.base_url && self.api_key == other.api_key
    }
}

impl Eq for MoonrakerClient {}

/// Klipper objects keyed by full object name (`extruder1`, `led cavity_led`).
/// Object field sets vary by printer configuration, so values stay open-ended
/// here and are decoded into typed views in `status.rs`.
pub(super) type KlipperObjects = BTreeMap<String, Map<String, Value>>;

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    result: T,
}

#[derive(Debug, Deserialize)]
pub(super) struct PrinterInfo {
    #[serde(default)]
    pub(super) hostname: String,
}

#[derive(Debug, Deserialize)]
struct SystemInfoEnvelope {
    system_info: SystemInfo,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct SystemInfo {
    #[serde(default)]
    pub(super) cpu_info: CpuInfo,
    #[serde(default)]
    pub(super) network: BTreeMap<String, NetworkInterface>,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct CpuInfo {
    #[serde(default)]
    pub(super) serial_number: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct NetworkInterface {
    #[serde(default)]
    pub(super) mac_address: String,
}

#[derive(Debug, Deserialize)]
struct ObjectList {
    objects: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ObjectQuery {
    status: KlipperObjects,
}

#[derive(Debug, Deserialize)]
struct WebcamList {
    webcams: Vec<Webcam>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Webcam {
    #[serde(default = "enabled_by_default")]
    pub(super) enabled: bool,
    #[serde(default)]
    pub(super) snapshot_url: String,
}

fn enabled_by_default() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub(super) struct UploadResult {
    pub(super) item: UploadedItem,
}

#[derive(Debug, Deserialize)]
pub(super) struct UploadedItem {
    pub(super) path: String,
}

/// A non-success Moonraker HTTP response, kept typed so link failures can
/// distinguish rejected API keys from other errors.
#[derive(Debug)]
pub struct MoonrakerHttpError {
    what: String,
    pub status: reqwest::StatusCode,
    body: String,
}

impl std::fmt::Display for MoonrakerHttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Moonraker {} failed with HTTP {}: {}",
            self.what, self.status, self.body
        )
    }
}

impl std::error::Error for MoonrakerHttpError {}

impl MoonrakerClient {
    pub fn new(host: &str, api_key: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("Moonraker HTTP client configuration is valid"),
            base_url: format!("http://{host}"),
            api_key: api_key.filter(|key| !key.trim().is_empty()),
        }
    }

    pub(crate) fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    /// The WebSocket JSON-RPC endpoint on the same host.
    pub(super) fn websocket_url(&self) -> String {
        format!(
            "ws://{}/websocket",
            self.base_url.trim_start_matches("http://")
        )
    }

    /// Resolves a Moonraker-relative URL (as returned by `server.webcams.list`)
    /// against the printer host; absolute URLs are returned unchanged.
    pub(super) fn resolve_url(&self, url: &str) -> String {
        if url.starts_with("http://") || url.starts_with("https://") {
            url.to_owned()
        } else if url.starts_with('/') {
            format!("{}{url}", self.base_url)
        } else {
            format!("{}/{url}", self.base_url)
        }
    }

    fn request(&self, method: Method, path: &str) -> RequestBuilder {
        let builder = self
            .http
            .request(method, format!("{}{path}", self.base_url))
            .timeout(REQUEST_TIMEOUT);
        match &self.api_key {
            Some(key) => builder.header(API_KEY_HEADER, key),
            None => builder,
        }
    }

    async fn send<T: DeserializeOwned>(
        &self,
        builder: RequestBuilder,
        what: &str,
    ) -> anyhow::Result<T> {
        let response = builder
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("send Moonraker {what} request"))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| format!("read Moonraker {what} response"))?;
        if !status.is_success() {
            return Err(MoonrakerHttpError {
                what: what.to_owned(),
                status,
                body: String::from_utf8_lossy(&body[..body.len().min(512)]).into_owned(),
            }
            .into());
        }
        serde_json::from_slice::<Envelope<T>>(&body)
            .map(|envelope| envelope.result)
            .with_context(|| format!("decode Moonraker {what} response"))
    }

    pub(super) async fn printer_info(&self) -> anyhow::Result<PrinterInfo> {
        self.send(self.request(Method::GET, "/printer/info"), "printer info")
            .await
    }

    pub(super) async fn system_info(&self) -> anyhow::Result<SystemInfo> {
        self.send::<SystemInfoEnvelope>(
            self.request(Method::GET, "/machine/system_info"),
            "system info",
        )
        .await
        .map(|envelope| envelope.system_info)
    }

    pub(super) async fn object_list(&self) -> anyhow::Result<Vec<String>> {
        self.send::<ObjectList>(
            self.request(Method::GET, "/printer/objects/list"),
            "object list",
        )
        .await
        .map(|list| list.objects)
    }

    pub(super) async fn query_objects(&self, objects: &[String]) -> anyhow::Result<KlipperObjects> {
        let query: BTreeMap<&str, Option<()>> =
            objects.iter().map(|name| (name.as_str(), None)).collect();
        self.send::<ObjectQuery>(
            self.request(Method::POST, "/printer/objects/query")
                .json(&serde_json::json!({ "objects": query })),
            "object query",
        )
        .await
        .map(|query| query.status)
    }

    pub(super) async fn webcams(&self) -> anyhow::Result<Vec<Webcam>> {
        self.send::<WebcamList>(
            self.request(Method::GET, "/server/webcams/list"),
            "webcam list",
        )
        .await
        .map(|list| list.webcams)
    }

    /// Uploads into the `gcodes` root under `directory`.
    pub(super) async fn upload_gcode(
        &self,
        directory: &str,
        filename: &str,
        contents: Vec<u8>,
    ) -> anyhow::Result<UploadResult> {
        let form = multipart::Form::new()
            .text("root", "gcodes")
            .text("path", directory.to_owned())
            .part(
                "file",
                multipart::Part::bytes(contents)
                    .file_name(filename.to_owned())
                    .mime_str("application/octet-stream")
                    .expect("static MIME type is valid"),
            );
        self.send(
            self.request(Method::POST, "/server/files/upload")
                .timeout(UPLOAD_TIMEOUT)
                .multipart(form),
            "file upload",
        )
        .await
    }

    pub(super) async fn start_print(&self, filename: &str) -> anyhow::Result<()> {
        self.send::<Value>(
            self.request(Method::POST, "/printer/print/start")
                .json(&serde_json::json!({ "filename": filename })),
            "print start",
        )
        .await
        .map(drop)
    }

    pub(super) async fn print_action(&self, action: PrintAction) -> anyhow::Result<()> {
        self.send::<Value>(self.request(Method::POST, action.path()), action.as_str())
            .await
            .map(drop)
    }

    pub(super) async fn run_gcode(&self, script: &str) -> anyhow::Result<()> {
        self.send::<Value>(
            self.request(Method::POST, "/printer/gcode/script")
                .json(&serde_json::json!({ "script": script })),
            "G-code script",
        )
        .await
        .map(drop)
    }

    /// Fetches a raw resource (camera snapshot) relative to the printer host.
    pub(super) async fn get_bytes(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let builder = self
            .http
            .get(self.resolve_url(url))
            .timeout(REQUEST_TIMEOUT);
        let builder = match &self.api_key {
            Some(key) => builder.header(API_KEY_HEADER, key),
            None => builder,
        };
        let response = builder
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .context("request Moonraker camera snapshot")?;
        let status = response.status();
        if !status.is_success() {
            bail!("Moonraker camera snapshot failed with HTTP {status}");
        }
        Ok(response
            .bytes()
            .await
            .map_err(reqwest::Error::without_url)
            .context("read Moonraker camera snapshot")?
            .to_vec())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PrintAction {
    Pause,
    Resume,
    Cancel,
}

impl PrintAction {
    fn path(self) -> &'static str {
        match self {
            Self::Pause => "/printer/print/pause",
            Self::Resume => "/printer/print/resume",
            Self::Cancel => "/printer/print/cancel",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Pause => "print pause",
            Self::Resume => "print resume",
            Self::Cancel => "print cancel",
        }
    }
}
