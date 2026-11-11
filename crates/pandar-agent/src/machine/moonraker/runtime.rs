use std::{collections::HashMap, sync::Mutex as StdMutex};

use anyhow::Context;
use pandar_core::PrinterOperation;
use pandar_protocol::agent::v1::{AgentEvent, PrintProjectFile};
use tokio::{sync::mpsc, task::JoinHandle};

use super::{
    MoonrakerCameraSource, MoonrakerClient, MoonrakerEndpoint, MoonrakerLinkRequest,
    camera::resolve_camera,
    identity::resolve_printer,
    operations::operate,
    print::print_gcode,
    projection::{endpoint_snapshot, machine_snapshot, materials_json},
    reports::forward_reports_with_retry,
    status::{KlipperStatus, ObjectSelection},
};
use crate::{
    AgentConfig,
    commands::authoritative_printer_snapshot_event,
    machine::{
        MachineSnapshot, MaterialRefreshResult, PrintProjectDispatchResult,
        PrinterOperationDispatchResult, PrinterRefreshResult,
    },
};

#[derive(Debug, Clone)]
struct LinkedPrinter {
    endpoint: MoonrakerEndpoint,
    client: MoonrakerClient,
}

/// Moonraker printers linked to this Agent and their status forwarders.
pub struct MoonrakerRuntime {
    config: AgentConfig,
    printers: tokio::sync::Mutex<HashMap<String, LinkedPrinter>>,
    report_tasks: tokio::sync::Mutex<HashMap<String, JoinHandle<()>>>,
    api_keys: StdMutex<Vec<String>>,
}

impl MoonrakerRuntime {
    pub fn new(config: AgentConfig, endpoints: Vec<MoonrakerEndpoint>) -> Self {
        let api_keys = endpoints
            .iter()
            .filter_map(|endpoint| endpoint.api_key.clone())
            .collect();
        let printers = endpoints
            .into_iter()
            .map(|endpoint| {
                let client = MoonrakerClient::new(&endpoint.host, endpoint.api_key.clone());
                (endpoint.serial.clone(), LinkedPrinter { endpoint, client })
            })
            .collect();
        Self {
            config,
            printers: tokio::sync::Mutex::new(printers),
            report_tasks: tokio::sync::Mutex::new(HashMap::new()),
            api_keys: StdMutex::new(api_keys),
        }
    }

    pub async fn contains(&self, serial: &str) -> bool {
        self.printers.lock().await.contains_key(serial)
    }

    pub fn redact(&self, message: &str) -> String {
        self.api_keys
            .lock()
            .unwrap()
            .iter()
            .filter(|key| !key.is_empty())
            .fold(message.to_owned(), |message, key| {
                message.replace(key.as_str(), "[REDACTED_API_KEY]")
            })
    }

    pub async fn prepare_session(&self, sender: &mpsc::Sender<AgentEvent>) -> anyhow::Result<()> {
        self.teardown_session().await;
        let printers: Vec<LinkedPrinter> = self.printers.lock().await.values().cloned().collect();
        for printer in printers {
            sender
                .send(authoritative_printer_snapshot_event(
                    &self.config,
                    endpoint_snapshot(&printer.endpoint, None),
                ))
                .await
                .with_context(|| {
                    format!(
                        "queue configured Moonraker printer {} snapshot",
                        printer.endpoint.serial
                    )
                })?;
            self.replace_report_task(printer, sender).await;
        }
        Ok(())
    }

    pub async fn teardown_session(&self) {
        for (_, task) in self.report_tasks.lock().await.drain() {
            task.abort();
        }
    }

    pub async fn link(
        &self,
        request: MoonrakerLinkRequest,
        sender: &mpsc::Sender<AgentEvent>,
    ) -> anyhow::Result<MachineSnapshot> {
        let resolved = resolve_printer(&request).await?;
        let status = query_status(&resolved.client, &resolved.selection).await?;
        let snapshot = machine_snapshot(&resolved.endpoint, &resolved.selection, &status);
        sender
            .send(authoritative_printer_snapshot_event(
                &self.config,
                snapshot.clone(),
            ))
            .await
            .context("queue linked Moonraker printer snapshot")?;
        if let Some(key) = &resolved.endpoint.api_key {
            self.api_keys.lock().unwrap().push(key.clone());
        }
        let printer = LinkedPrinter {
            endpoint: resolved.endpoint,
            client: resolved.client,
        };
        self.printers
            .lock()
            .await
            .insert(printer.endpoint.serial.clone(), printer.clone());
        self.replace_report_task(printer, sender).await;
        Ok(snapshot)
    }

    pub async fn validate(&self, serial: &str) -> anyhow::Result<()> {
        self.printer(serial).await.map(drop)
    }

    pub async fn refresh(&self) -> Vec<PrinterRefreshResult> {
        let printers: Vec<LinkedPrinter> = self.printers.lock().await.values().cloned().collect();
        let mut results = Vec::with_capacity(printers.len());
        for printer in printers {
            let result = async {
                let selection = selection(&printer.client).await?;
                let status = query_status(&printer.client, &selection).await?;
                anyhow::Ok((selection, status))
            }
            .await;
            results.push(match result {
                Ok((selection, status)) => PrinterRefreshResult {
                    snapshot: machine_snapshot(&printer.endpoint, &selection, &status),
                    materials: materials_json(&selection, &status, &pandar_core::created_at_now())
                        .map(|printer_materials_json| MaterialRefreshResult {
                            serial: printer.endpoint.serial.clone(),
                            printer_id: None,
                            printer_materials_json,
                        }),
                },
                Err(error) => {
                    tracing::warn!(
                        serial = %printer.endpoint.serial,
                        error = %self.redact(&format!("{error:#}")),
                        "Moonraker printer refresh failed"
                    );
                    PrinterRefreshResult {
                        snapshot: endpoint_snapshot(&printer.endpoint, Some("offline")),
                        materials: None,
                    }
                }
            });
        }
        results
    }

    pub async fn refresh_materials(
        &self,
        serial: &str,
        printer_id: Option<&str>,
    ) -> anyhow::Result<MaterialRefreshResult> {
        let printer = self.printer(serial).await?;
        let selection = selection(&printer.client).await?;
        let status = query_status(&printer.client, &selection).await?;
        let printer_materials_json =
            materials_json(&selection, &status, &pandar_core::created_at_now())
                .context("printer does not report filament channels")?;
        Ok(MaterialRefreshResult {
            serial: serial.to_owned(),
            printer_id: printer_id.map(str::to_owned),
            printer_materials_json,
        })
    }

    pub async fn print(
        &self,
        serial: &str,
        command: &PrintProjectFile,
        artifact: Vec<u8>,
    ) -> anyhow::Result<PrintProjectDispatchResult> {
        let printer = self.printer(serial).await?;
        print_gcode(&printer.client, command, artifact).await
    }

    pub async fn operate(
        &self,
        serial: &str,
        operation: PrinterOperation,
    ) -> anyhow::Result<PrinterOperationDispatchResult> {
        let printer = self.printer(serial).await?;
        let selection = selection(&printer.client).await?;
        operate(&printer.client, &selection, operation).await?;
        Ok(PrinterOperationDispatchResult::dispatched())
    }

    pub async fn camera(&self, serial: &str) -> anyhow::Result<MoonrakerCameraSource> {
        let printer = self.printer(serial).await?;
        resolve_camera(&printer.client).await
    }

    async fn printer(&self, serial: &str) -> anyhow::Result<LinkedPrinter> {
        self.printers
            .lock()
            .await
            .get(serial)
            .cloned()
            .with_context(|| format!("no Moonraker printer configured for serial {serial}"))
    }

    async fn replace_report_task(&self, printer: LinkedPrinter, sender: &mpsc::Sender<AgentEvent>) {
        let serial = printer.endpoint.serial.clone();
        let task = tokio::spawn(forward_reports_with_retry(
            self.config.clone(),
            printer.client,
            printer.endpoint,
            sender.clone(),
        ));
        if let Some(previous) = self.report_tasks.lock().await.insert(serial, task) {
            previous.abort();
        }
    }
}

async fn selection(client: &MoonrakerClient) -> anyhow::Result<ObjectSelection> {
    Ok(ObjectSelection::from_available(
        &client.object_list().await.context("list Klipper objects")?,
    ))
}

async fn query_status(
    client: &MoonrakerClient,
    selection: &ObjectSelection,
) -> anyhow::Result<KlipperStatus> {
    let mut status = KlipperStatus::default();
    status.apply(
        client
            .query_objects(&selection.subscribed)
            .await
            .context("query Klipper status")?,
    );
    Ok(status)
}
