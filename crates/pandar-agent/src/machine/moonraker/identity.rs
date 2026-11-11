use anyhow::{Context, bail};

use super::{
    MoonrakerClient, MoonrakerEndpoint, MoonrakerLinkRequest, client::SystemInfo,
    status::ObjectSelection,
};

pub(super) const KLIPPER_MODEL: &str = "Klipper";

/// A reachable Moonraker printer resolved to a stable Pandar identity.
pub(super) struct ResolvedPrinter {
    pub(super) endpoint: MoonrakerEndpoint,
    pub(super) client: MoonrakerClient,
    pub(super) selection: ObjectSelection,
}

pub(super) async fn resolve_printer(
    request: &MoonrakerLinkRequest,
) -> anyhow::Result<ResolvedPrinter> {
    let client = MoonrakerClient::new(&request.host, request.api_key.clone());
    let info = client
        .printer_info()
        .await
        .with_context(|| format!("query Moonraker printer info at {}", request.host))?;
    let system = client
        .system_info()
        .await
        .with_context(|| format!("query Moonraker system info at {}", request.host))?;
    let serial = stable_serial(&system).with_context(|| {
        format!(
            "Moonraker at {} reports neither a CPU serial nor a network MAC address",
            request.host
        )
    })?;
    if let Some(expected) = request.expected_serial.as_deref()
        && expected != serial
    {
        bail!(
            "printer at {} reported identity {serial}, expected {expected}",
            request.host
        );
    }
    let objects = client
        .object_list()
        .await
        .with_context(|| format!("list Klipper objects at {}", request.host))?;
    let selection = ObjectSelection::from_available(&objects);
    let name = request
        .name
        .clone()
        .or_else(|| Some(info.hostname.trim().to_owned()).filter(|name| !name.is_empty()));

    Ok(ResolvedPrinter {
        endpoint: MoonrakerEndpoint {
            host: request.host.clone(),
            serial,
            api_key: request.api_key.clone(),
            model: Some(KLIPPER_MODEL.to_owned()),
            name,
        },
        client,
        selection,
    })
}

/// The CPU serial when the SoC exposes one, otherwise the MAC address of the
/// first physical interface. Hostnames are not unique across a fleet.
fn stable_serial(system: &SystemInfo) -> Option<String> {
    let cpu = system.cpu_info.serial_number.trim();
    if !cpu.is_empty() && cpu.chars().any(|c| c != '0') {
        return Some(cpu.to_ascii_uppercase());
    }
    system
        .network
        .iter()
        .filter(|(name, _)| name.as_str() != "lo")
        .map(|(_, interface)| interface.mac_address.trim().replace([':', '-'], ""))
        .find(|mac| !mac.is_empty() && mac.chars().any(|c| c != '0'))
        .map(|mac| mac.to_ascii_uppercase())
}
