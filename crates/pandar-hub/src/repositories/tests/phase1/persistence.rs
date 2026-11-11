use super::*;

#[tokio::test]
async fn file_sqlite_records_survive_reconnect() {
    let temp_dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite://{}", temp_dir.path().join("pandar.db").display());

    let config = DatabaseConfig::from_url(&url).unwrap();
    let database = Database::connect(&config).await.unwrap();
    database.migrate().await.unwrap();
    let tenants = TenantRepository::new(database.clone());
    let agents = AgentRepository::new(database.clone());
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    agents.create(tenant.id, "agent").await.unwrap();
    drop(database);

    let database = Database::connect(&config).await.unwrap();
    database.migrate().await.unwrap();
    assert_eq!(
        TenantRepository::new(database.clone())
            .count()
            .await
            .unwrap(),
        1
    );
    assert_eq!(AgentRepository::new(database).count().await.unwrap(), 1);
}
