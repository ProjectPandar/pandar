use std::time::Duration;

use anyhow::{Context, bail};
use futures_util::StreamExt;
use serde::Deserialize;

use pandar_core::PrinterFamily;

use crate::{
    AgentConfig,
    commands::parse_printer_config,
    machine::{BambuPrinterEndpoint, moonraker::MoonrakerEndpoint},
};

/// A printer connection saved in pandar-hub for this Agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SavedPrinter {
    Bambu(BambuPrinterEndpoint),
    Moonraker(MoonrakerEndpoint),
}

impl SavedPrinter {
    pub(crate) fn serial(&self) -> &str {
        match self {
            Self::Bambu(endpoint) => &endpoint.serial,
            Self::Moonraker(endpoint) => &endpoint.serial,
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct StartupPrinters {
    pub(crate) bambu: Vec<BambuPrinterEndpoint>,
    pub(crate) moonraker: Vec<MoonrakerEndpoint>,
}

pub(crate) async fn startup_printers(config: &AgentConfig) -> anyhow::Result<StartupPrinters> {
    let mut printers = StartupPrinters {
        bambu: parse_printer_config(&config.printers)?,
        moonraker: Vec::new(),
    };
    let Some(hub_api_url) = config
        .hub_api_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(printers);
    };

    let saved = fetch_saved_printer_connections(config, hub_api_url)
        .await
        .context("load saved printer connections from pandar-hub")?;
    for printer in saved {
        printers
            .bambu
            .retain(|configured| configured.serial != printer.serial());
        match printer {
            SavedPrinter::Bambu(endpoint) => printers.bambu.push(endpoint),
            SavedPrinter::Moonraker(endpoint) => printers.moonraker.push(endpoint),
        }
    }

    Ok(printers)
}

pub(crate) async fn fetch_saved_printer_connections(
    config: &AgentConfig,
    hub_api_url: &str,
) -> anyhow::Result<Vec<SavedPrinter>> {
    let url = format!(
        "{}/api/v1/agents/{}/printers",
        hub_api_url.trim_end_matches('/'),
        config.agent_id
    );
    let response = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("saved printer HTTP client configuration is valid")
        .get(url)
        .bearer_auth(&config.agent_credential)
        .send()
        .await
        .context("request saved printer connections")?;
    let status = response.status();
    if !status.is_success() {
        bail!("hub saved printer connection request failed with HTTP {status}");
    }

    const MAX_SAVED_PRINTER_RESPONSE_BYTES: usize = 1024 * 1024;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SAVED_PRINTER_RESPONSE_BYTES as u64)
    {
        bail!(
            "hub saved printer connection response exceeds {MAX_SAVED_PRINTER_RESPONSE_BYTES} bytes"
        );
    }
    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk
            .map_err(reqwest::Error::without_url)
            .context("read saved printer connection response")?;
        if body.len().saturating_add(chunk.len()) > MAX_SAVED_PRINTER_RESPONSE_BYTES {
            bail!(
                "hub saved printer connection response exceeds {MAX_SAVED_PRINTER_RESPONSE_BYTES} bytes"
            );
        }
        body.extend_from_slice(&chunk);
    }
    let response = serde_json::from_slice::<SavedPrinterConnectionsResponse>(&body)
        .context("decode saved printer connections")?;
    response
        .printers
        .into_iter()
        .map(saved_printer_endpoint)
        .collect()
}

#[derive(Debug, Deserialize)]
struct SavedPrinterConnectionsResponse {
    printers: Vec<SavedPrinterConnection>,
}

#[derive(Debug, Deserialize)]
struct SavedPrinterConnection {
    serial: String,
    host: String,
    printer_family: PrinterFamily,
    access_code: Option<String>,
    name: String,
    model: Option<String>,
}

fn saved_printer_endpoint(value: SavedPrinterConnection) -> anyhow::Result<SavedPrinter> {
    validate_required("serial", &value.serial)?;
    validate_required("host", &value.host)?;
    Ok(match value.printer_family {
        PrinterFamily::BambuLab => {
            let access_code = value.access_code.unwrap_or_default();
            validate_required("access_code", &access_code)?;
            SavedPrinter::Bambu(BambuPrinterEndpoint {
                host: value.host,
                serial: value.serial,
                access_code,
                model: value.model,
                name: Some(value.name),
            })
        }
        PrinterFamily::Moonraker => SavedPrinter::Moonraker(MoonrakerEndpoint {
            host: value.host,
            serial: value.serial,
            api_key: value.access_code.filter(|key| !key.trim().is_empty()),
            model: value.model,
            name: Some(value.name),
        }),
    })
}

fn validate_required(field: &str, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() {
        bail!("hub saved printer connection has missing or blank {field}");
    }

    Ok(())
}
