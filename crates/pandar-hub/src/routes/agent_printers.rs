use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use pandar_core::PrinterFamily;
use serde::Serialize;

use crate::{AppState, routes::ApiError};

pub(in crate::routes) async fn list_agent_printers(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentPrinterConnectionsResponse>, ApiError> {
    let agent_id = pandar_core::AgentId::parse(&agent_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid_agent_id"))?;
    let authorized = crate::routes::agent_auth::authorize_agent(&state, &headers, agent_id).await?;
    let printers = state
        .printers()
        .list_for_tenant(authorized.tenant_id)
        .await?
        .into_iter()
        .filter(|printer| printer.agent_id == agent_id)
        .filter_map(|printer| {
            let host = non_blank(printer.host)?;
            let access_code = non_blank(printer.access_code);
            if printer.printer_family == PrinterFamily::BambuLab && access_code.is_none() {
                return None;
            }
            Some(AgentPrinterConnectionResponse {
                serial: printer.serial_number,
                printer_family: printer.printer_family,
                host,
                access_code,
                name: printer.name,
                model: printer.model,
            })
        })
        .collect();

    Ok(Json(AgentPrinterConnectionsResponse { printers }))
}

fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
}

#[derive(Debug, Serialize)]
pub(in crate::routes) struct AgentPrinterConnectionsResponse {
    printers: Vec<AgentPrinterConnectionResponse>,
}

#[derive(Debug, Serialize)]
struct AgentPrinterConnectionResponse {
    serial: String,
    printer_family: PrinterFamily,
    host: String,
    access_code: Option<String>,
    name: String,
    model: Option<String>,
}
