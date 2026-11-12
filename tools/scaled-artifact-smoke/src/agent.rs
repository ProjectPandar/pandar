use std::{
    net::SocketAddr,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use anyhow::{Context, bail};
use pandar_core::AgentStatus;
use pandar_hub::{AppState, db::Database, entities::agents, grpc::AgentControlService};
use pandar_protocol::agent::v1::{
    AgentEvent, AgentHeartbeat, AgentHello, CommandAck, HubCommand, PrintJobReport,
    PrintProjectFile, PrinterSnapshot, agent_control_client::AgentControlClient,
    agent_control_server::AgentControlServer, agent_event, hub_command,
};
use sea_orm::EntityTrait;
use tokio::sync::mpsc;
use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};

use crate::fixture::SmokeFixture;

const SETTLE_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// An agent that speaks the real `ReverseConnect` gRPC stream to one Hub
/// replica. Scenarios drive the Hub only through this stream and the HTTP
/// router, never through Hub-internal handlers.
pub struct FakeAgent {
    tenant_id: pandar_core::TenantId,
    agent_id: pandar_core::AgentId,
    serial: String,
    pub printer_id: String,
    database: Database,
    events: mpsc::Sender<AgentEvent>,
    commands: Option<tonic::Streaming<HubCommand>>,
    next_event_id: AtomicU64,
}

impl FakeAgent {
    /// Connects to `state` and announces the fixture printer through it.
    pub async fn connect(
        state: &AppState,
        database: &Database,
        fixture: &SmokeFixture,
    ) -> anyhow::Result<Self> {
        let grpc_url = serve_grpc(state.clone()).await?;
        let mut client = AgentControlClient::connect(grpc_url)
            .await
            .context("connect smoke agent gRPC client")?;
        let (events, receiver) = mpsc::channel(16);
        let mut agent = Self {
            tenant_id: fixture.tenant_id,
            agent_id: fixture.agent_id,
            serial: fixture.printer_serial.clone(),
            printer_id: String::new(),
            database: database.clone(),
            events,
            commands: None,
            next_event_id: AtomicU64::new(0),
        };
        // The Hub answers the stream only after it has read the hello.
        agent
            .send(agent_event::Event::Hello(AgentHello {
                name: "scaled-smoke-agent".to_owned(),
                version: "scaled-smoke".to_owned(),
                credential: fixture.agent_credential.clone(),
                capabilities: Vec::new(),
            }))
            .await?;
        agent.commands = Some(
            client
                .reverse_connect(ReceiverStream::new(receiver))
                .await
                .context("open smoke agent reverse-connect stream")?
                .into_inner(),
        );
        agent.printer_id = agent.announce_printer(state).await?;
        Ok(agent)
    }

    async fn announce_printer(&self, state: &AppState) -> anyhow::Result<String> {
        self.send_snapshot("idle").await?;
        let deadline = tokio::time::Instant::now() + SETTLE_TIMEOUT;
        loop {
            let printer = state
                .printers()
                .list_for_tenant(self.tenant_id)
                .await?
                .into_iter()
                .find(|printer| printer.serial_number == self.serial);
            if let Some(printer) = printer {
                return Ok(printer.id);
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("Hub did not persist the announced printer snapshot");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    pub async fn send_snapshot(&self, printer_state: &str) -> anyhow::Result<()> {
        self.send(agent_event::Event::PrinterSnapshot(PrinterSnapshot {
            serial: self.serial.clone(),
            name: format!("Scaled Smoke Printer {}", self.serial),
            state: printer_state.to_owned(),
            model: "X1C".to_owned(),
            connection_authoritative: true,
            telemetry_authoritative: true,
            ..PrinterSnapshot::default()
        }))
        .await
    }

    /// Waits for the next `PrintProjectFile` command, skipping other commands.
    pub async fn next_print_command(
        &mut self,
        timeout: Duration,
    ) -> anyhow::Result<(pandar_core::CommandId, PrintProjectFile)> {
        let deadline = tokio::time::Instant::now() + timeout;
        let commands = self
            .commands
            .as_mut()
            .context("agent stream is not connected")?;
        loop {
            let command = tokio::time::timeout_at(deadline, commands.message())
                .await
                .context("print command did not reach the agent stream")?
                .context("agent command stream failed")?
                .context("agent command stream closed before a print command")?;
            if let Some(hub_command::Command::PrintProjectFile(print)) = command.command {
                let command_id = pandar_core::CommandId::parse(&command.command_id)?;
                return Ok((command_id, print));
            }
        }
    }

    pub async fn acknowledge(&self, command_id: pandar_core::CommandId) -> anyhow::Result<()> {
        self.send(agent_event::Event::CommandAck(CommandAck {
            command_id: command_id.to_string(),
            accepted: true,
            error: String::new(),
        }))
        .await
    }

    pub async fn report(&self, print: &PrintProjectFile, gcode_state: &str) -> anyhow::Result<()> {
        let finished = gcode_state == "FINISH";
        self.send(agent_event::Event::PrintJobReport(PrintJobReport {
            serial: self.serial.clone(),
            job_id: print.job_id.clone(),
            artifact_id: print.artifact_id.clone(),
            gcode_state: gcode_state.to_owned(),
            percent: if finished { 100 } else { 42 },
            has_percent: true,
            remaining_time_minutes: if finished { 0 } else { 60 },
            has_remaining_time_minutes: true,
            current_layer: 3,
            has_current_layer: true,
            total_layers: 9,
            has_total_layers: true,
            observed_at: pandar_core::created_at_now(),
            ..PrintJobReport::default()
        }))
        .await
    }

    /// Returns once the Hub has handled every event sent so far. Non-snapshot
    /// events are handled in stream order, so a heartbeat is a barrier.
    pub async fn settle(&self) -> anyhow::Result<()> {
        let observed_at = pandar_core::created_at_now();
        self.send(agent_event::Event::Heartbeat(AgentHeartbeat {
            observed_at: observed_at.clone(),
        }))
        .await?;
        let deadline = tokio::time::Instant::now() + SETTLE_TIMEOUT;
        loop {
            let agent = agents::Entity::find_by_id(self.agent_id.to_string())
                .one(&self.database.sea_orm_connection())
                .await?;
            if agent.is_some_and(|agent| agent.last_seen_at.as_deref() == Some(&observed_at)) {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("Hub did not settle the agent event stream");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    /// Closes the stream and waits until `state` has marked the agent offline.
    pub async fn disconnect(self, state: &AppState) -> anyhow::Result<()> {
        let Self {
            agent_id,
            events,
            commands,
            ..
        } = self;
        drop(events);
        if let Some(mut commands) = commands {
            while commands.message().await.ok().flatten().is_some() {}
        }
        let deadline = tokio::time::Instant::now() + SETTLE_TIMEOUT;
        loop {
            let agent = state.agents().get(agent_id).await?;
            if agent.is_some_and(|agent| agent.status == AgentStatus::Offline) {
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("Hub did not mark the disconnected agent offline");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    async fn send(&self, event: agent_event::Event) -> anyhow::Result<()> {
        let event_id = self.next_event_id.fetch_add(1, Ordering::Relaxed);
        self.events
            .send(AgentEvent {
                agent_id: self.agent_id.to_string(),
                tenant_id: self.tenant_id.to_string(),
                event_id: format!("scaled-smoke-{event_id}"),
                event: Some(event),
            })
            .await
            .context("smoke agent event stream closed")
    }
}

async fn serve_grpc(state: AppState) -> anyhow::Result<String> {
    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        if let Err(err) = tonic::transport::Server::builder()
            .add_service(AgentControlServer::new(AgentControlService::new(state)))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
        {
            eprintln!("scaled smoke hub gRPC server failed: {err:#}");
        }
    });
    Ok(format!("http://{addr}"))
}
