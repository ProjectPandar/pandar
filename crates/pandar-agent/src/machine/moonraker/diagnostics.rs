use anyhow::Context;

use crate::machine::diagnostics::{
    DiagnosticCheck, DiagnosticStatus, PrinterDiagnosticResult, check,
};

use super::{
    MoonrakerClient, MoonrakerEndpoint,
    camera::resolve_camera,
    reports::redact,
    status::{KlipperStatus, Webhooks},
    websocket::probe_websocket,
};

/// Checks a linked Moonraker printer over the surfaces Pandar uses: the HTTP
/// API, Klipper's state, the status WebSocket and the camera.
pub(super) async fn diagnose(
    client: &MoonrakerClient,
    endpoint: &MoonrakerEndpoint,
    agent_version: &str,
) -> PrinterDiagnosticResult {
    let mut checks = vec![check(
        "configured_printer",
        DiagnosticStatus::Ok,
        "Printer is configured on this agent.",
        None,
    )];
    let http = match client.printer_info().await {
        Ok(_) => check(
            "moonraker_http",
            DiagnosticStatus::Ok,
            format!("Moonraker HTTP API answered at {}.", client.base_url()),
            None,
        ),
        Err(error) => problem(
            "moonraker_http",
            "Moonraker HTTP API is not reachable.",
            error,
            endpoint,
        ),
    };
    let reachable = http.status == DiagnosticStatus::Ok;
    checks.push(http);
    if !reachable {
        for id in ["klippy_state", "websocket", "camera"] {
            checks.push(check(
                id,
                DiagnosticStatus::Skipped,
                "Skipped because Moonraker HTTP reachability failed first.",
                None,
            ));
        }
    } else {
        checks.push(klippy_state_check(client, endpoint).await);
        checks.push(match probe_websocket(client, agent_version).await {
            Ok(()) => check(
                "websocket",
                DiagnosticStatus::Ok,
                "Moonraker status WebSocket accepted the subscription.",
                None,
            ),
            Err(error) => problem(
                "websocket",
                "Moonraker status WebSocket handshake failed.",
                error,
                endpoint,
            ),
        });
        checks.push(match resolve_camera(client).await {
            Ok(camera) => check(
                "camera",
                DiagnosticStatus::Ok,
                format!(
                    "Camera snapshot is served from {}.",
                    camera.client.base_url()
                ),
                None,
            ),
            Err(error) => {
                let details = redact(&format!("{error:#}"), endpoint);
                check(
                    "camera",
                    DiagnosticStatus::Warning,
                    "No webcam is reachable from this agent.",
                    Some(details),
                )
            }
        });
    }

    PrinterDiagnosticResult::new(
        endpoint.serial.clone(),
        Some(endpoint.host.clone()),
        endpoint.model.clone(),
        checks,
        None,
    )
}

fn problem(
    id: &'static str,
    message: &str,
    error: anyhow::Error,
    endpoint: &MoonrakerEndpoint,
) -> DiagnosticCheck {
    let details = redact(&format!("{error:#}"), endpoint);
    tracing::warn!(
        serial = %endpoint.serial,
        error = %details,
        "Moonraker printer diagnostic {id} check failed"
    );
    check(id, DiagnosticStatus::Problem, message, Some(details))
}

async fn klippy_state_check(
    client: &MoonrakerClient,
    endpoint: &MoonrakerEndpoint,
) -> DiagnosticCheck {
    let result = async {
        let mut status = KlipperStatus::default();
        status.apply(
            client
                .query_objects(&["webhooks".to_owned()])
                .await
                .context("query Klipper webhooks state")?,
        );
        anyhow::Ok(status.object::<Webhooks>("webhooks"))
    }
    .await;
    match result {
        Ok(webhooks) => {
            let message = non_blank(&webhooks.state_message);
            match webhooks.state.as_str() {
                "ready" => check(
                    "klippy_state",
                    DiagnosticStatus::Ok,
                    "Klipper is ready.",
                    None,
                ),
                "startup" => check(
                    "klippy_state",
                    DiagnosticStatus::Warning,
                    "Klipper is still starting.",
                    message,
                ),
                _ => check(
                    "klippy_state",
                    DiagnosticStatus::Problem,
                    format!("Klipper is {}.", webhooks.state),
                    message,
                ),
            }
        }
        Err(error) => problem(
            "klippy_state",
            "Klipper state could not be read.",
            error,
            endpoint,
        ),
    }
}

fn non_blank(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}
