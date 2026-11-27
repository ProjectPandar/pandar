use std::{collections::BTreeMap, time::Duration};

mod control;

use anyhow::Context;

pub(super) use control::PrintAction;
use reqwest::{Method, RequestBuilder, multipart};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value};

use super::http_error::MoonrakerHttpError;

const API_KEY_HEADER: &str = "X-Api-Key";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(600);

/// Thin typed client for the Moonraker HTTP API.
#[derive(Debug, Clone)]
pub struct MoonrakerClient {
    http: reqwest::Client,
    /// Printer host as configured, possibly with an explicit `:port`.
    host: String,
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
    /// Firmware such as Snapmaker's describes the machine itself here; stock
    /// Moonraker has no `product_info`.
    #[serde(default)]
    pub(super) product_info: ProductInfo,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct ProductInfo {
    #[serde(default)]
    pub(super) machine_type: String,
    #[serde(default)]
    pub(super) device_name: String,
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
    #[serde(default)]
    pub(super) stream_url: String,
}

#[derive(Debug, Default, Deserialize)]
pub(super) struct FileMetadata {
    #[serde(default)]
    pub(super) estimated_time: Option<f64>,
}

fn enabled_by_default() -> bool {
    true
}

async fn send_resource(builder: RequestBuilder) -> anyhow::Result<reqwest::Response> {
    let response = builder.send().await.map_err(reqwest::Error::without_url)?;
    let status = response.status();
    if !status.is_success() {
        return Err(MoonrakerHttpError {
            what: "camera resource".to_owned(),
            status,
            body: String::new(),
        }
        .into());
    }
    Ok(response)
}

#[derive(Debug, Deserialize)]
pub(super) struct UploadResult {
    pub(super) item: UploadedItem,
}

#[derive(Debug, Deserialize)]
pub(super) struct UploadedItem {
    pub(super) path: String,
}

impl MoonrakerClient {
    pub fn new(host: &str, api_key: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("Moonraker HTTP client configuration is valid"),
            host: host.to_owned(),
            base_url: format!("http://{host}"),
            api_key: api_key.filter(|key| !key.trim().is_empty()),
        }
    }

    pub(crate) fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    /// A client for the same host on the default HTTP port, where the
    /// Fluidd/Mainsail nginx proxies webcam streams. `None` when the configured
    /// host already omits the port.
    pub(super) fn default_port_client(&self) -> Option<Self> {
        self.host
            .contains(':')
            .then(|| Self::new(self.bare_host(), self.api_key.clone()))
    }

    /// The configured printer host, without any explicit port.
    fn bare_host(&self) -> &str {
        self.host
            .split_once(':')
            .map_or(&self.host, |(host, _)| host)
    }

    /// The base URL camera resources resolve against.
    pub(super) fn base_url(&self) -> &str {
        &self.base_url
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

    /// The WebSocket JSON-RPC endpoint on the same host.
    pub(super) fn websocket_url(&self) -> String {
        format!(
            "ws://{}/websocket",
            self.base_url.trim_start_matches("http://")
        )
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

    /// The G-code commands Klipper currently registers, keyed by command.
    pub(super) async fn gcode_help(&self) -> anyhow::Result<BTreeMap<String, String>> {
        self.send(
            self.request(Method::GET, "/printer/gcode/help"),
            "G-code help",
        )
        .await
    }

    pub(super) async fn file_metadata(&self, path: &str) -> anyhow::Result<FileMetadata> {
        self.send(
            self.request(Method::POST, "/server/files/metadata")
                .json(&serde_json::json!({ "filename": path })),
            "file metadata",
        )
        .await
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

    /// Fetches a raw resource (camera snapshot) relative to the printer host.
    pub(super) async fn get_bytes(&self, url: &str) -> anyhow::Result<Vec<u8>> {
        let response = send_resource(self.get(url).timeout(REQUEST_TIMEOUT))
            .await
            .context("request Moonraker camera snapshot")?;
        Ok(response
            .bytes()
            .await
            .map_err(reqwest::Error::without_url)
            .context("read Moonraker camera snapshot")?
            .to_vec())
    }

    /// Opens a long-lived GET on a printer-relative resource such as an MJPEG
    /// webcam stream. Without a total timeout, the response stays open for as
    /// long as the camera session runs.
    pub(super) async fn open_stream(&self, url: &str) -> anyhow::Result<reqwest::Response> {
        send_resource(self.get(url))
            .await
            .context("open Moonraker camera stream")
    }

    fn get(&self, url: &str) -> RequestBuilder {
        let builder = self.http.get(self.resolve_url(url));
        match &self.api_key {
            Some(key) => builder.header(API_KEY_HEADER, key),
            None => builder,
        }
    }
}
