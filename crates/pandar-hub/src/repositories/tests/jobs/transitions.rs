use super::*;

#[tokio::test]
async fn job_repository_create_rolls_back_command_when_job_insert_fails() {
    let (database, tenants, agents, _, commands, jobs) = repositories().await;
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();
    let printer_id =
        crate::repositories::test_helpers::insert_printer_fixture(&database, tenant.id, agent.id)
            .await
            .unwrap();

    let err = jobs
        .create_print_job(create_input(tenant.id, agent.id, &printer_id, ""))
        .await
        .unwrap_err();

    assert!(matches!(err, RepositoryError::Database(_)));
    assert_eq!(commands.count().await.unwrap(), 0);
}
