use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Subcommand)]
pub(crate) enum ClientCommand {
    /// Inspect the currently authenticated API key or CLI session
    Whoami,
    /// Send a raw HTTP request to Pandar Hub
    Request(RequestArgs),
    /// Inspect and control printers
    #[command(subcommand)]
    Printers(PrintersCommand),
    /// Inspect and submit print jobs
    #[command(subcommand)]
    Jobs(JobsCommand),
    /// Inspect and trigger agent actions
    #[command(subcommand)]
    Agents(AgentsCommand),
    /// Manage tenant API keys (tokens)
    #[command(subcommand)]
    Tokens(TokensCommand),
    /// Inspect asynchronous commands
    #[command(subcommand)]
    Commands(CommandsCommand),
}

#[derive(Debug, Args)]
pub(crate) struct RequestArgs {
    /// HTTP method (GET, POST, PATCH, PUT, DELETE)
    pub method: String,
    /// API path (paths starting with /api/ are sent as-is; others are tenant-scoped)
    pub path: String,
    /// Inline JSON request body
    #[arg(short = 'd', long, conflicts_with = "data_file")]
    pub data: Option<String>,
    /// File path containing the request body (`-` reads stdin)
    #[arg(long)]
    pub data_file: Option<PathBuf>,
    /// Extra HTTP header (`NAME: VALUE`, repeatable; cannot override Authorization/Host/Cookie)
    #[arg(short = 'H', long = "header")]
    pub headers: Vec<String>,
    /// Query parameter (`KEY=VALUE`, repeatable)
    #[arg(short = 'q', long = "query")]
    pub query: Vec<String>,
    /// Send request without Bearer authentication (for public endpoints)
    #[arg(long)]
    pub no_auth: bool,
    /// Print HTTP status and redacted response headers to stderr
    #[arg(long)]
    pub include: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum PrintersCommand {
    /// List printers in the active tenant
    List,
    /// Get details for a single printer
    Get { printer_id: String },
    /// Send a control operation to a printer
    Control {
        printer_id: String,
        #[arg(long)]
        action: String,
        /// Optional JSON object merged with `action`
        #[arg(long)]
        payload: Option<String>,
    },
    /// Request an AMS/material state refresh
    RefreshMaterials { printer_id: String },
    /// Delete a printer (requires tenant admin)
    Delete { printer_id: String },
}

#[derive(Debug, Subcommand)]
pub(crate) enum JobsCommand {
    /// List print jobs in the active tenant
    List,
    /// Get details for a single print job
    Get { job_id: String },
    /// Upload a print artifact and create a print job
    Create(CreateJobArgs),
    /// Retry dispatching a failed or stalled print job
    Retry {
        job_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Reprint a completed or failed print job
    Reprint {
        job_id: String,
        #[arg(long)]
        reason: Option<String>,
    },
    /// Delete a print job (requires tenant admin)
    Delete { job_id: String },
}

#[derive(Debug, Args)]
pub(crate) struct CreateJobArgs {
    pub printer_id: String,
    /// Path to the `.3mf` or `.gcode` artifact file
    #[arg(long)]
    pub file: PathBuf,
    /// Override the uploaded filename (defaults to file basename)
    #[arg(long)]
    pub filename: Option<String>,
    /// Override MIME content type (defaults by extension)
    #[arg(long)]
    pub content_type: Option<String>,
    /// Plate ID inside the 3MF project
    #[arg(long, default_value_t = 1)]
    pub plate: i64,
    /// Use AMS during print
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
    pub use_ams: bool,
    /// Enable bed leveling
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub bed_leveling: bool,
    /// Auto bed leveling calibration mode (0, 1, or 2)
    #[arg(long, default_value_t = 0)]
    pub auto_bed_leveling: u8,
    /// Enable flow calibration
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
    pub flow_cali: bool,
    /// Auto flow calibration mode (0, 1, or 2)
    #[arg(long, default_value_t = 0)]
    pub auto_flow_cali: u8,
    /// Auto offset calibration mode (0, 1, or 2)
    #[arg(long, default_value_t = 0)]
    pub auto_offset_cali: u8,
    /// Enable timelapse recording
    #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
    pub timelapse: bool,
    /// Optional JSON `ams_mapping` payload
    #[arg(long)]
    pub ams_mapping: Option<String>,
    /// Optional JSON `ams_mapping2` payload
    #[arg(long)]
    pub ams_mapping2: Option<String>,
    /// Optional JSON `ams_mapping_info` payload
    #[arg(long)]
    pub ams_mapping_info: Option<String>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum AgentsCommand {
    /// List agents in the active tenant
    List,
    /// Queue a printer state refresh on an agent
    RefreshPrinters { agent_id: String },
    /// Discover LAN printers through an agent
    DiscoverPrinters {
        agent_id: String,
        #[arg(long)]
        timeout_seconds: Option<u32>,
    },
    /// Run printer diagnostics through an agent
    DiagnosePrinter {
        agent_id: String,
        #[arg(long)]
        serial: String,
    },
}

#[derive(Debug, Subcommand)]
pub(crate) enum TokensCommand {
    /// List tenant API keys (tokens)
    List,
    /// Create a new tenant API key (token)
    Create {
        #[arg(long)]
        name: String,
        /// Comma-separated scopes (e.g. *, operator, viewer, agent:register; empty = read-only)
        #[arg(long, default_value = "*")]
        scopes: String,
        /// Optional RFC 3339 expiration timestamp
        #[arg(long)]
        expires_at: Option<String>,
    },
    /// Rotate an existing tenant API key
    Rotate {
        token_id: String,
        #[arg(long)]
        expires_at: Option<String>,
    },
    /// Revoke a tenant API key
    Revoke { token_id: String },
}

#[derive(Debug, Subcommand)]
pub(crate) enum CommandsCommand {
    /// Fetch status and result of a queued command
    Get { command_id: String },
}
