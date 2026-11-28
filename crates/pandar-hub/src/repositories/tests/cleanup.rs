use super::*;
use crate::cleanup::{CleanupMode, CleanupOptions, cleanup_database};

pub(in crate::repositories::tests) mod lifecycle;
mod retention;
pub(super) mod storage;

use retention::{insert_join_link, insert_tenant_token, insert_ticket};
use storage::RecordingArtifactStorage;

#[tokio::test]
async fn cleanup_dry_run_does_not_mutate_and_execute_preserves_active_jobs() {
    let (database, tenants, agents, _, commands, jobs) = repositories().await;
    exercise_cleanup(database, tenants, agents, commands, jobs).await;
}

pub(super) async fn exercise_cleanup(
    database: Database,
    tenants: TenantRepository,
    agents: AgentRepository,
    commands: CommandRepository,
    jobs: JobRepository,
) {
    let tenant = tenants.create("cleanup", "Cleanup").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();
    let terminal = jobs
        .create_print_job(crate::repositories::tests::jobs::create_input(
            tenant.id,
            agent.id,
            &printer_id,
            "terminal-artifact",
        ))
        .await
        .unwrap();
    jobs.mark_print_sent(terminal.job.command_id, tenant.id, agent.id)
        .await
        .unwrap();
    jobs.mark_print_succeeded(terminal.job.command_id, tenant.id, agent.id)
        .await
        .unwrap();
    jobs.apply_print_report(crate::repositories::tests::jobs::report_input(
        tenant.id,
        agent.id,
        &printer_id,
        Some(terminal.job.id),
        None,
        "FINISH",
    ))
    .await
    .unwrap();
    let active = jobs
        .create_print_job(crate::repositories::tests::jobs::create_input(
            tenant.id,
            agent.id,
            &printer_id,
            "active-artifact",
        ))
        .await
        .unwrap();
    let shared_terminal = jobs
        .create_print_job(crate::repositories::tests::jobs::create_input(
            tenant.id,
            agent.id,
            &printer_id,
            "shared-terminal-artifact",
        ))
        .await
        .unwrap();
    jobs.mark_print_sent(shared_terminal.job.command_id, tenant.id, agent.id)
        .await
        .unwrap();
    jobs.mark_print_succeeded(shared_terminal.job.command_id, tenant.id, agent.id)
        .await
        .unwrap();
    jobs.apply_print_report(crate::repositories::tests::jobs::report_input(
        tenant.id,
        agent.id,
        &printer_id,
        Some(shared_terminal.job.id),
        None,
        "FINISH",
    ))
    .await
    .unwrap();
    let shared_active = jobs
        .create_print_job(crate::repositories::tests::jobs::create_input(
            tenant.id,
            agent.id,
            &printer_id,
            "shared-active-artifact",
        ))
        .await
        .unwrap();

    point_job_at_artifact(
        &database,
        &shared_terminal.job.id.to_string(),
        &terminal.artifact.id,
    )
    .await;
    point_job_at_artifact(
        &database,
        &shared_active.job.id.to_string(),
        &active.artifact.id,
    )
    .await;

    make_old(
        &database,
        &terminal.job.id.to_string(),
        &terminal.job.command_id.to_string(),
    )
    .await;
    make_old(
        &database,
        &shared_terminal.job.id.to_string(),
        &shared_terminal.job.command_id.to_string(),
    )
    .await;

    let storage = RecordingArtifactStorage::default();
    let dry_run = cleanup_database(
        &database,
        Some(&storage),
        CleanupOptions::default(),
        CleanupMode::DryRun,
    )
    .await
    .unwrap();
    assert_eq!(dry_run.jobs, 2);
    assert_eq!(dry_run.artifacts, 1);
    assert_eq!(dry_run.artifact_bytes, 42);
    assert_eq!(dry_run.artifact_storage_paths.len(), 1);
    assert!(
        jobs.get_for_tenant(tenant.id, terminal.job.id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(commands.count().await.unwrap(), 4);

    assert!(storage.deleted().is_empty());

    let executed = cleanup_database(
        &database,
        Some(&storage),
        CleanupOptions::default(),
        CleanupMode::Execute,
    )
    .await
    .unwrap();
    assert_eq!(executed.jobs, 2);
    assert_eq!(executed.artifacts, 1);
    assert_eq!(executed.artifact_bytes, 42);
    assert_eq!(
        executed.artifact_storage_paths,
        dry_run.artifact_storage_paths
    );
    assert_eq!(storage.deleted(), dry_run.artifact_storage_paths);
    assert_eq!(artifact_count(&database).await, 3);
    assert!(
        jobs.get_for_tenant(tenant.id, terminal.job.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        jobs.get_for_tenant(tenant.id, shared_terminal.job.id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        jobs.get_for_tenant(tenant.id, active.job.id)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        jobs.get_for_tenant(tenant.id, shared_active.job.id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(commands.count().await.unwrap(), 2);
}

#[tokio::test]
async fn cleanup_uses_terminal_time_for_tokens_and_tickets() {
    let database = sqlite_database().await;
    insert_ticket(
        &database,
        "old-used",
        "2025-01-01T00:00:00Z",
        Some("2025-01-02T00:00:00Z"),
        None,
        "2026-01-01T00:00:00Z",
    )
    .await;
    insert_ticket(
        &database,
        "new-used",
        "2025-01-01T00:00:00Z",
        Some("2999-01-02T00:00:00Z"),
        None,
        "3000-01-01T00:00:00Z",
    )
    .await;
    insert_tenant_token(
        &database,
        "old-revoked",
        "2025-01-01T00:00:00Z",
        Some("2025-01-02T00:00:00Z"),
        None,
    )
    .await;
    insert_tenant_token(
        &database,
        "new-revoked",
        "2025-01-01T00:00:00Z",
        Some("2999-01-02T00:00:00Z"),
        None,
    )
    .await;

    let dry_run = cleanup_database(
        &database,
        None,
        CleanupOptions::default(),
        CleanupMode::DryRun,
    )
    .await
    .unwrap();

    assert_eq!(dry_run.plugin_login_tickets, 1);
    assert_eq!(dry_run.tenant_tokens, 1);
}

#[tokio::test]
async fn auth_retention_cleanup_purges_tokens_and_join_links_past_seven_day_window() {
    exercise_auth_retention_cleanup(sqlite_database().await).await;
}

pub(in crate::repositories::tests) async fn exercise_auth_retention_cleanup(database: Database) {
    let now = time::OffsetDateTime::now_utc();
    let fmt = |offset: time::Duration| {
        (now + offset)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap()
    };
    let created_old = fmt(-time::Duration::days(60));
    let future_10d = fmt(time::Duration::days(10));
    let future_30d = fmt(time::Duration::days(30));
    let past_3d = fmt(-time::Duration::days(3));
    let past_8d = fmt(-time::Duration::days(8));
    let cutoff_7d = fmt(-time::Duration::days(7));

    insert_tenant_token(&database, "active-never", &created_old, None, None).await;
    insert_tenant_token(
        &database,
        "active-future",
        &created_old,
        None,
        Some(&future_10d),
    )
    .await;
    insert_tenant_token(
        &database,
        "recent-expired",
        &created_old,
        None,
        Some(&past_3d),
    )
    .await;
    insert_tenant_token(
        &database,
        "recent-revoked",
        &created_old,
        Some(&past_3d),
        None,
    )
    .await;
    insert_tenant_token(
        &database,
        "stale-expired",
        &created_old,
        None,
        Some(&past_8d),
    )
    .await;
    insert_tenant_token(
        &database,
        "stale-revoked",
        &created_old,
        Some(&past_8d),
        Some(&future_30d),
    )
    .await;

    insert_join_link(&database, "join-active", &created_old, None, &future_10d).await;
    insert_join_link(
        &database,
        "join-recent-expired",
        &created_old,
        None,
        &past_3d,
    )
    .await;
    insert_join_link(
        &database,
        "join-recent-revoked",
        &created_old,
        Some(&past_3d),
        &future_30d,
    )
    .await;
    insert_join_link(
        &database,
        "join-stale-expired",
        &created_old,
        None,
        &past_8d,
    )
    .await;
    insert_join_link(
        &database,
        "join-stale-revoked",
        &created_old,
        Some(&past_8d),
        &future_30d,
    )
    .await;

    let dry_run = cleanup_database(
        &database,
        None,
        CleanupOptions::default(),
        CleanupMode::DryRun,
    )
    .await
    .unwrap();
    assert_eq!(dry_run.tenant_tokens, 2);
    assert_eq!(dry_run.join_links, 2);

    let auth = AuthRepository::new(database.clone());
    let deleted_tokens = auth
        .purge_expired_or_revoked_tenant_tokens(&cutoff_7d)
        .await
        .unwrap();
    assert_eq!(deleted_tokens, 2);
    let deleted_links = auth
        .purge_expired_or_revoked_join_links(&cutoff_7d)
        .await
        .unwrap();
    assert_eq!(deleted_links, 2);

    let remaining_tokens: std::collections::BTreeSet<String> = match &database {
        Database::Sqlite(pool) => sqlx::query_scalar::<_, String>("SELECT id FROM tenant_tokens")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .collect(),
        Database::Postgres(pool) => sqlx::query_scalar::<_, String>("SELECT id FROM tenant_tokens")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .collect(),
    };
    assert_eq!(
        remaining_tokens,
        std::collections::BTreeSet::from([
            "active-never".to_owned(),
            "active-future".to_owned(),
            "recent-expired".to_owned(),
            "recent-revoked".to_owned(),
        ])
    );

    let remaining_links: std::collections::BTreeSet<String> = match &database {
        Database::Sqlite(pool) => sqlx::query_scalar::<_, String>("SELECT id FROM join_links")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .collect(),
        Database::Postgres(pool) => sqlx::query_scalar::<_, String>("SELECT id FROM join_links")
            .fetch_all(pool)
            .await
            .unwrap()
            .into_iter()
            .collect(),
    };
    assert_eq!(
        remaining_links,
        std::collections::BTreeSet::from([
            "join-active".to_owned(),
            "join-recent-expired".to_owned(),
            "join-recent-revoked".to_owned(),
        ])
    );
}

async fn make_old(database: &Database, job_id: &str, command_id: &str) {
    let old = "2025-01-01T00:00:00Z";
    match database {
        Database::Sqlite(pool) => {
            sqlx::query("UPDATE jobs SET updated_at = ?1 WHERE id = ?2")
                .bind(old)
                .bind(job_id)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("UPDATE commands SET updated_at = ?1 WHERE id = ?2")
                .bind(old)
                .bind(command_id)
                .execute(pool)
                .await
                .unwrap();
        }
        Database::Postgres(pool) => {
            sqlx::query("UPDATE jobs SET updated_at = $1 WHERE id = $2")
                .bind(old)
                .bind(job_id)
                .execute(pool)
                .await
                .unwrap();
            sqlx::query("UPDATE commands SET updated_at = $1 WHERE id = $2")
                .bind(old)
                .bind(command_id)
                .execute(pool)
                .await
                .unwrap();
        }
    }
}

async fn point_job_at_artifact(database: &Database, job_id: &str, artifact_id: &str) {
    match database {
        Database::Sqlite(pool) => {
            sqlx::query("UPDATE jobs SET artifact_id = ?1 WHERE id = ?2")
                .bind(artifact_id)
                .bind(job_id)
                .execute(pool)
                .await
                .unwrap();
        }
        Database::Postgres(pool) => {
            sqlx::query("UPDATE jobs SET artifact_id = $1 WHERE id = $2")
                .bind(artifact_id)
                .bind(job_id)
                .execute(pool)
                .await
                .unwrap();
        }
    }
}

async fn artifact_count(database: &Database) -> i64 {
    match database {
        Database::Sqlite(pool) => sqlx::query_scalar("SELECT COUNT(*) FROM job_artifacts")
            .fetch_one(pool)
            .await
            .unwrap(),
        Database::Postgres(pool) => sqlx::query_scalar("SELECT COUNT(*) FROM job_artifacts")
            .fetch_one(pool)
            .await
            .unwrap(),
    }
}
