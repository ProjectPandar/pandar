use super::*;
use crate::repositories::ArtifactQuotaLimits;

#[tokio::test]
async fn concurrent_artifact_quota_check_allows_only_one_insert() {
    let (database, tenants, agents, _, _, jobs) = repositories().await;
    let tenant = tenants.create("quota", "Quota").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();
    let actor = || crate::repositories::AuditActor {
        actor_type: "system".to_owned(),
        user_id: None,
        metadata: None,
    };
    let quota = ArtifactQuotaLimits {
        tenant_bytes: 42,
        tenant_count: 1,
        global_bytes: 42,
        global_count: 1,
    };
    let first = jobs.create_print_job_with_quota_and_audit(
        create_input(tenant.id, agent.id, &printer_id, "quota-artifact-1"),
        quota,
        actor(),
    );
    let second = jobs.create_print_job_with_quota_and_audit(
        create_input(tenant.id, agent.id, &printer_id, "quota-artifact-2"),
        quota,
        actor(),
    );
    let (first, second) = tokio::join!(first, second);
    let outcomes = [first, second];

    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|result| matches!(result, Err(RepositoryError::ArtifactQuotaExceeded)))
            .count(),
        1
    );
}

#[tokio::test]
async fn job_repository_rejects_wrong_tenant_printer() {
    let (database, tenants, agents, _, _, jobs) = repositories().await;
    let acme = tenants.create("acme", "Acme Labs").await.unwrap();
    let beta = tenants.create("beta", "Beta Labs").await.unwrap();
    let agent = agents.create(acme.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, acme.id, agent.id)
            .await
            .unwrap();

    let err = jobs
        .create_print_job(create_input(beta.id, agent.id, &printer_id, "artifact-1"))
        .await
        .unwrap_err();

    assert!(matches!(err, RepositoryError::MissingPrinter));
}
