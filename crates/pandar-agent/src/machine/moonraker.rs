//! Klipper printers reached through the Moonraker HTTP / WebSocket API.
//!
//! Support is generic over Klipper objects. Optional firmware extensions are
//! detected by object presence, e.g. `print_task_config` filament channels.

mod camera;
mod client;
mod identity;
mod operations;
mod print;
mod projection;
mod reports;
mod runtime;
mod status;
#[cfg(test)]
mod tests;

pub use client::{MoonrakerClient, MoonrakerHttpError};
pub use runtime::MoonrakerRuntime;

/// A linked Moonraker printer. `host` may carry an explicit `:port`; without
/// one Moonraker is reached on port 80, where Fluidd/Mainsail nginx proxies it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoonrakerEndpoint {
    pub host: String,
    pub serial: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub name: Option<String>,
}

/// A request to (re)link a Moonraker printer. The serial is derived from the
/// printer itself; `expected_serial` pins it when reloading a saved connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoonrakerLinkRequest {
    pub host: String,
    pub api_key: Option<String>,
    pub name: Option<String>,
    pub expected_serial: Option<String>,
}

/// A Moonraker printer camera, resolved from `server.webcams.list`. Frames are
/// polled from the snapshot URL, which every Moonraker webcam service exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoonrakerCameraSource {
    pub client: MoonrakerClient,
    pub snapshot_url: String,
}
