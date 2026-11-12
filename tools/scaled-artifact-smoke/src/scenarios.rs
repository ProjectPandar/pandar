use std::{collections::HashSet, sync::Arc, time::Duration};

use anyhow::{Context, ensure};
use axum::http::StatusCode;
use pandar_core::{CommandStatus, PrintStatus};
use pandar_hub::{AppState, db::Database, entities::machine_events};
use pandar_protocol::agent::v1::PrintProjectFile;
use sea_orm::{EntityTrait, PaginatorTrait};

use crate::{
    agent::FakeAgent,
    fixture::{SmokeFixture, SmokeWorld, seed_fixture, world_root},
    harness::HarnessConfig,
    http::{
        connect_ws_with_ticket, create_print, create_print_through_multipart_route,
        create_ws_ticket, download_artifact, download_artifact_route, next_ws_event_type,
        serve_hub,
    },
    storage::{FailingObjectStorage, FailureMode, shared_object_storage},
};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn artifact_dispatch_download(
    iteration: usize,
    config: &HarnessConfig,
) -> anyhow::Result<()> {
    let world = SmokeWorld::for_config(config).await?;
    let fixture = seed_fixture(
        &world.hub_a,
        &config.fixture_suffix("artifact", iteration, 0),
    )
    .await?;
    let mut agent = world.agent(&world.hub_b, &fixture).await?;

    create_print_through_multipart_route(&world.hub_a, &fixture, &agent.printer_id).await?;
    let print = receive_sent_print(&world.hub_b, &fixture, &mut agent).await?;
    ensure!(
        !print.artifact_download_path.trim().is_empty(),
        "PrintProjectFile is missing artifact_download_path"
    );
    ensure!(
        !print.storage_path.contains("pandar-spool"),
        "smoke command used a shared spool path"
    );

    download_artifact(&world.hub_b, &fixture, &print.artifact_download_path).await?;
    concurrent_plugin_pressure(iteration, config).await?;
    agent.disconnect(&world.hub_b).await
}

pub async fn websocket_fanout(iteration: usize, config: &HarnessConfig) -> anyhow::Result<()> {
    let world = SmokeWorld::for_config(config).await?;
    let fixture =
        seed_fixture(&world.hub_a, &config.fixture_suffix("fanout", iteration, 0)).await?;
    let mut agent = world.agent(&world.hub_a, &fixture).await?;
    create_print_through_multipart_route(&world.hub_a, &fixture, &agent.printer_id).await?;
    let (command_id, print) = agent.next_print_command(COMMAND_TIMEOUT).await?;
    agent.acknowledge(command_id).await?;
    agent.settle().await?;

    let tickets = futures_util::future::try_join_all(
        (0..config.concurrency)
            .map(|_| create_ws_ticket(&world.hub_a, fixture.tenant_id, &fixture.tenant_token)),
    )
    .await?;
    let base_url = serve_hub(world.hub_b.clone()).await?;
    let mut sockets = futures_util::future::try_join_all(
        tickets
            .iter()
            .map(|ticket| connect_ws_with_ticket(&base_url, fixture.tenant_id, ticket)),
    )
    .await?;

    agent.send_snapshot("running").await?;
    agent.report(&print, "RUNNING").await?;

    for socket in &mut sockets {
        let mut seen = HashSet::new();
        while !(seen.contains("printer_snapshot") && seen.contains("job_progress")) {
            let event_type = next_ws_event_type(socket)
                .await
                .with_context(|| format!("websocket subscriber only received {seen:?}"))?;
            seen.insert(event_type);
        }
    }
    agent.disconnect(&world.hub_a).await
}

pub async fn restart_convergence(iteration: usize, config: &HarnessConfig) -> anyhow::Result<()> {
    let world = SmokeWorld::for_config(config).await?;
    let fixture = seed_fixture(
        &world.hub_a,
        &config.fixture_suffix("restart", iteration, 0),
    )
    .await?;
    // The print is queued while its agent is disconnected, so only a replica
    // that reloads the shared database can deliver it.
    let agent = world.agent(&world.hub_a, &fixture).await?;
    let printer_id = agent.printer_id.clone();
    agent.disconnect(&world.hub_a).await?;
    create_print_through_multipart_route(&world.hub_a, &fixture, &printer_id).await?;

    let restarted = world.restarted_state().await?;
    let mut agent = world.agent(&restarted, &fixture).await?;
    receive_sent_print(&restarted, &fixture, &mut agent).await?;

    let ticket = create_ws_ticket(&world.hub_a, fixture.tenant_id, &fixture.tenant_token).await?;
    let base_url = serve_hub(world.hub_a.clone()).await?;
    let mut socket = connect_ws_with_ticket(&base_url, fixture.tenant_id, &ticket).await?;
    // Both directions must converge: a print accepted by the surviving replica
    // wakes the agent on the restarted one, and a snapshot the restarted
    // replica applies reaches subscribers of the surviving one.
    create_print_through_multipart_route(&world.hub_a, &fixture, &agent.printer_id).await?;
    receive_sent_print(&restarted, &fixture, &mut agent).await?;
    agent.send_snapshot("running").await?;
    loop {
        let event_type = next_ws_event_type(&mut socket)
            .await
            .context("surviving replica websocket did not receive printer snapshot")?;
        if event_type == "printer_snapshot" {
            break;
        }
    }
    agent.disconnect(&restarted).await
}

pub async fn storage_failures(iteration: usize, config: &HarnessConfig) -> anyhow::Result<()> {
    let root = world_root(config)?;
    let storage = FailingObjectStorage::new(shared_object_storage(root.join("objects"))?);
    let world = SmokeWorld::dry_run(&root, Arc::new(storage.clone())).await?;
    let fixture = seed_fixture(&world.hub_a, &format!("storage-{iteration}")).await?;
    let mut agent = world.agent(&world.hub_b, &fixture).await?;

    storage.set_mode(FailureMode::Put);
    let (status, _) = create_print(&world.hub_a, &fixture, &agent.printer_id).await?;
    ensure!(
        status != StatusCode::CREATED,
        "put failure unexpectedly created print"
    );
    ensure!(
        world.hub_a.commands().count().await? == 0,
        "put failure created command row"
    );
    ensure!(
        world
            .hub_a
            .jobs()
            .list_for_tenant(fixture.tenant_id)
            .await?
            .is_empty(),
        "put failure created job row"
    );

    storage.set_mode(FailureMode::None);
    create_print_through_multipart_route(&world.hub_a, &fixture, &agent.printer_id).await?;
    let (_, print) = agent.next_print_command(COMMAND_TIMEOUT).await?;
    storage.set_mode(FailureMode::Open);
    let (status, body) =
        download_artifact_route(&world.hub_b, &fixture, &print.artifact_download_path).await?;
    ensure!(
        status == StatusCode::BAD_GATEWAY
            && String::from_utf8_lossy(&body).contains("artifact_unavailable"),
        "open failure did not surface artifact_unavailable: {status} {}",
        String::from_utf8_lossy(&body)
    );

    storage.set_mode(FailureMode::Delete);
    let delete_err = world
        .hub_a
        .artifact_storage()
        .delete_artifact(&print.storage_path)
        .await
        .expect_err("delete failure should reject delete");
    ensure!(
        format!("{delete_err:#}").contains("injected artifact delete failure"),
        "delete failure did not preserve context: {delete_err:#}"
    );
    agent.disconnect(&world.hub_b).await
}

async fn concurrent_plugin_pressure(
    iteration: usize,
    config: &HarnessConfig,
) -> anyhow::Result<()> {
    let world = SmokeWorld::for_config(config).await?;
    let mut clients = Vec::with_capacity(config.concurrency);
    for index in 0..config.concurrency {
        let fixture = seed_fixture(
            &world.hub_a,
            &config.fixture_suffix("pressure", iteration, index),
        )
        .await?;
        let agent = world.agent(&world.hub_b, &fixture).await?;
        clients.push((fixture, agent));
    }
    futures_util::future::try_join_all(clients.iter().map(|(fixture, agent)| {
        create_print_through_multipart_route(&world.hub_a, fixture, &agent.printer_id)
    }))
    .await?;

    let mut prints = Vec::with_capacity(clients.len());
    for (_, agent) in &mut clients {
        prints.push(agent.next_print_command(COMMAND_TIMEOUT).await?.1);
    }
    let artifacts = prints
        .iter()
        .map(|print| print.artifact_id.as_str())
        .collect::<HashSet<_>>();
    ensure!(
        artifacts.len() == config.concurrency,
        "concurrent plugin clients did not each receive their own artifact"
    );
    for index in [0, clients.len() - 1] {
        download_artifact(
            &world.hub_b,
            &clients[index].0,
            &prints[index].artifact_download_path,
        )
        .await?;
    }
    for (_, agent) in clients {
        agent.disconnect(&world.hub_b).await?;
    }
    Ok(())
}

pub async fn terminal_report_idempotence(
    iteration: usize,
    config: &HarnessConfig,
) -> anyhow::Result<()> {
    let world = SmokeWorld::for_config(config).await?;
    let fixture = seed_fixture(
        &world.hub_a,
        &config.fixture_suffix("terminal", iteration, 0),
    )
    .await?;
    let mut agent = world.agent(&world.hub_a, &fixture).await?;
    create_print_through_multipart_route(&world.hub_a, &fixture, &agent.printer_id).await?;
    let (command_id, print) = agent.next_print_command(COMMAND_TIMEOUT).await?;
    agent.acknowledge(command_id).await?;

    agent.report(&print, "FINISH").await?;
    agent.settle().await?;
    ensure!(
        print_status(&world.hub_a, &fixture, &print).await? == PrintStatus::Completed,
        "terminal report did not complete the print"
    );
    let terminal_event_count = machine_event_count(&world.database).await?;
    ensure!(
        terminal_event_count > 0,
        "terminal report recorded no machine event"
    );

    agent.report(&print, "FINISH").await?;
    agent.settle().await?;
    ensure!(
        machine_event_count(&world.database).await? == terminal_event_count,
        "terminal replay inserted another machine event"
    );

    agent.report(&print, "RUNNING").await?;
    agent.settle().await?;
    ensure!(
        print_status(&world.hub_a, &fixture, &print).await? == PrintStatus::Completed,
        "stale RUNNING report regressed terminal print status"
    );
    agent.disconnect(&world.hub_a).await
}

pub async fn nats_reconnect(iteration: usize, config: &HarnessConfig) -> anyhow::Result<()> {
    let world = SmokeWorld::for_config(config).await?;
    let fixture = seed_fixture(
        &world.hub_a,
        &config.fixture_suffix("nats-reconnect", iteration, 0),
    )
    .await?;
    let mut agent = world.agent(&world.hub_b, &fixture).await?;

    println!("WAIT scenario=nats-reconnect iteration={iteration} stop-restart-nats-now");
    tokio::time::sleep(reconnect_pause("PANDAR_SOAK_NATS_RECONNECT_PAUSE_SECS")).await;

    create_print_through_multipart_route(&world.hub_a, &fixture, &agent.printer_id).await?;
    receive_sent_print(&world.hub_b, &fixture, &mut agent).await?;
    agent.disconnect(&world.hub_b).await
}

pub async fn postgres_reconnect(iteration: usize, config: &HarnessConfig) -> anyhow::Result<()> {
    let world = SmokeWorld::for_config(config).await?;
    let fixture = seed_fixture(
        &world.hub_a,
        &config.fixture_suffix("postgres-reconnect", iteration, 0),
    )
    .await?;
    let mut agent = world.agent(&world.hub_b, &fixture).await?;

    println!("WAIT scenario=postgres-reconnect iteration={iteration} stop-restart-postgres-now");
    tokio::time::sleep(reconnect_pause("PANDAR_SOAK_POSTGRES_RECONNECT_PAUSE_SECS")).await;

    create_print_through_multipart_route(&world.hub_a, &fixture, &agent.printer_id).await?;
    let print = receive_sent_print(&world.hub_b, &fixture, &mut agent).await?;
    agent.report(&print, "FINISH").await?;
    agent.settle().await?;
    ensure!(
        print_status(&world.hub_b, &fixture, &print).await? == PrintStatus::Completed,
        "PostgreSQL reconnect terminal report did not complete the print"
    );
    agent.disconnect(&world.hub_b).await
}

fn reconnect_pause(variable: &str) -> Duration {
    std::env::var(variable)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| *seconds > 0)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(10))
}

/// Receives the next print command, checks that the delivering replica
/// persisted it as sent, and acknowledges it.
async fn receive_sent_print(
    state: &AppState,
    fixture: &SmokeFixture,
    agent: &mut FakeAgent,
) -> anyhow::Result<PrintProjectFile> {
    let (command_id, print) = agent.next_print_command(COMMAND_TIMEOUT).await?;
    let persisted = state
        .commands()
        .get_for_tenant(fixture.tenant_id, command_id)
        .await?
        .context("expected persisted command after dispatch")?;
    ensure!(
        persisted.status == CommandStatus::Sent,
        "delivered command is {:?}, expected sent",
        persisted.status
    );
    agent.acknowledge(command_id).await?;
    agent.settle().await?;
    Ok(print)
}

async fn print_status(
    state: &AppState,
    fixture: &SmokeFixture,
    print: &PrintProjectFile,
) -> anyhow::Result<PrintStatus> {
    let job = state
        .jobs()
        .get_for_tenant(fixture.tenant_id, pandar_core::JobId::parse(&print.job_id)?)
        .await?
        .context("expected print job")?;
    Ok(job.job.print.status)
}

async fn machine_event_count(database: &Database) -> anyhow::Result<u64> {
    machine_events::Entity::find()
        .count(&database.sea_orm_connection())
        .await
        .context("count machine events")
}
