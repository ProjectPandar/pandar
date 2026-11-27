//! Print lifecycle and G-code control over the Moonraker HTTP API.

use std::time::Duration;

use anyhow::Context;
use reqwest::Method;
use serde_json::Value;

use super::MoonrakerClient;

/// How long a control waits for Klipper's answer before the Agent reports
/// the script dispatched and leaves the request running in the background.
/// Immediate failures (unknown command, printer not ready) answer well within
/// this; a tool change that homes first or a filament feed runs far longer.
const GCODE_GRACE: Duration = Duration::from_secs(3);
/// Bound on a backgrounded script; Moonraker itself waits for Klipper
/// indefinitely, so this only stops a request from leaking forever.
const GCODE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::machine::moonraker) enum PrintAction {
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

impl MoonrakerClient {
    pub(in crate::machine::moonraker) async fn start_print(
        &self,
        filename: &str,
    ) -> anyhow::Result<()> {
        self.send::<Value>(
            self.request(Method::POST, "/printer/print/start")
                .json(&serde_json::json!({ "filename": filename })),
            "print start",
        )
        .await
        .map(drop)
    }

    pub(in crate::machine::moonraker) async fn print_action(
        &self,
        action: PrintAction,
    ) -> anyhow::Result<()> {
        self.send::<Value>(self.request(Method::POST, action.path()), action.as_str())
            .await
            .map(drop)
    }

    /// Runs a script and waits up to `GCODE_GRACE` for Klipper to answer, so
    /// immediate errors surface to the caller. A script still running after
    /// that keeps going in the background; its eventual outcome is logged and
    /// shows up in telemetry rather than in the command result.
    pub(in crate::machine::moonraker) async fn run_gcode(
        &self,
        script: &str,
    ) -> anyhow::Result<()> {
        let client = self.clone();
        let script = script.to_owned();
        let task_script = script.clone();
        let mut request = tokio::spawn(async move { client.send_gcode(&task_script).await });
        match tokio::time::timeout(GCODE_GRACE, &mut request).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => return Err(error),
            Ok(Err(error)) => {
                return Err(error).context("join Moonraker G-code request");
            }
            Err(_) => {
                tracing::info!(
                    script,
                    "Moonraker G-code still running; dispatched in the background"
                );
                tokio::spawn(async move {
                    match request.await {
                        Ok(Ok(())) => {
                            tracing::info!(script, "background Moonraker G-code finished")
                        }
                        Ok(Err(error)) => tracing::warn!(
                            script,
                            error = %format!("{error:#}"),
                            "background Moonraker G-code failed"
                        ),
                        Err(error) => tracing::warn!(
                            script,
                            error = %format!("{error:#}"),
                            "background Moonraker G-code task panicked"
                        ),
                    }
                });
            }
        }
        Ok(())
    }

    async fn send_gcode(&self, script: &str) -> anyhow::Result<()> {
        self.send::<Value>(
            self.request(Method::POST, "/printer/gcode/script")
                .timeout(GCODE_TIMEOUT)
                .json(&serde_json::json!({ "script": script })),
            "G-code script",
        )
        .await
        .map(drop)
    }
}
