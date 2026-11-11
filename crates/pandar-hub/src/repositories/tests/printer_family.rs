use pandar_core::PrinterFamily;

use super::*;

#[tokio::test]
async fn sqlite_moonraker_printer_family_round_trips() {
    assert_moonraker_family_round_trip(sqlite_database().await).await;
}

#[tokio::test]
async fn postgres_moonraker_printer_family_round_trips_when_configured() {
    let Some(database) = postgres::postgres_database().await else {
        eprintln!("skipping PostgreSQL test; PANDAR_TEST_POSTGRES_URL is not set");
        return;
    };
    assert_moonraker_family_round_trip((*database).clone()).await;
}

/// A keyless Moonraker link persists its family; later non-authoritative
/// telemetry (which defaults to the Bambu wire value) must not flip it.
async fn assert_moonraker_family_round_trip(database: Database) {
    let tenants = TenantRepository::new(database.clone());
    let agents = AgentRepository::new(database.clone());
    let printers = PrinterRepository::new(database);
    let tenant = tenants.create("acme", "Acme Labs").await.unwrap();
    let agent = agents.create(tenant.id, "agent").await.unwrap();

    let linked = printers
        .upsert_snapshot(
            tenant.id,
            agent.id,
            snapshot(PrinterFamily::Moonraker, Some("192.0.2.40:7125"), true),
        )
        .await
        .unwrap();
    assert_eq!(linked.printer_family, PrinterFamily::Moonraker);
    assert_eq!(linked.host.as_deref(), Some("192.0.2.40:7125"));
    assert_eq!(linked.access_code, None);

    printers
        .upsert_snapshot(
            tenant.id,
            agent.id,
            snapshot(PrinterFamily::BambuLab, None, false),
        )
        .await
        .unwrap();
    let stored = printers
        .get_for_tenant(tenant.id, &linked.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.printer_family, PrinterFamily::Moonraker);
    assert_eq!(stored.host.as_deref(), Some("192.0.2.40:7125"));
}

fn snapshot(
    printer_family: PrinterFamily,
    host: Option<&str>,
    connection_authoritative: bool,
) -> PrinterSnapshotUpsert {
    PrinterSnapshotUpsert {
        printer_family,
        serial_number: "A1B2C3D4E5F60718".to_owned(),
        host: host.map(str::to_owned),
        access_code: None,
        name: "Workshop U1".to_owned(),
        model: Some("Klipper".to_owned()),
        status: Some("IDLE".to_owned()),
        observed_at: "2026-11-11T00:00:00Z".to_owned(),
        nozzle_temperatures: Vec::new(),
        active_nozzle: None,
        bed_temperature_celsius: None,
        bed_target_temperature_celsius: None,
        chamber_temperature_celsius: None,
        chamber_target_temperature_celsius: None,
        chamber_light_on: None,
        cooling_system: None,
        nozzle_system: None,
        connection_authoritative,
        telemetry_authoritative: true,
    }
}
