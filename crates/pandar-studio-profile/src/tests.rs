use super::*;

#[test]
fn resolves_four_part_studio_versions_by_first_three_components() {
    assert_eq!(
        resolve_studio_version("02.07.01.62").unwrap().id,
        "02.07.01"
    );
    assert_eq!(resolve_studio_version("2.7.1.62").unwrap().id, "02.07.01");
    assert_eq!(
        resolve_studio_version("02.07.01.99").unwrap().id,
        "02.07.01"
    );
    assert_eq!(
        resolve_studio_version("02.08.01.55").unwrap().id,
        "02.08.01"
    );
    assert!(resolve_studio_version("02.09.00.00").is_err());
    assert!(resolve_studio_version("02.07.01").is_err());
}

#[test]
fn resolves_studio_2_8_2_release_contract() {
    let latest = resolve_studio_version("02.08.02.61").unwrap();

    assert_eq!(latest.id, "02.08.02");
    assert_eq!(
        latest.studio_commit,
        "926a7192574bcb9b3a732e1ec59a46d79cb45466"
    );
    assert_eq!(latest.reference_network_agent_version, "02.08.02.54");
    assert_eq!(latest.reported_network_agent_version, "02.08.02.99");
    assert_eq!(latest.network_exports, 110);
    assert_eq!(latest.file_transfer_exports, 21);
    assert!(latest.capabilities.print_queue_plate_id);
    assert!(latest.capabilities.slot_mappings_sync);
    let previous = abi_series("02.08.01").unwrap();
    assert!(!previous.capabilities.print_queue_plate_id);
    assert!(!previous.capabilities.slot_mappings_sync);
    assert_eq!(
        latest.native_modes(),
        ["version", "bind", "print", "ams", "slot-mappings", "ft"]
    );
}

#[test]
fn selects_native_modes_by_abi_capabilities() {
    assert_eq!(
        abi_series("02.08.00").unwrap().native_modes(),
        ["version", "bind", "print", "ft"]
    );
    assert_eq!(
        abi_series("02.08.01").unwrap().native_modes(),
        ["version", "bind", "print", "ams", "ft"]
    );
}

#[test]
fn selects_release_assets_by_abi_series() {
    assert_eq!(
        abi_series("02.07.01").unwrap().hook_bundle_name(),
        "pandar-studio-hook-02.07.01-windows-amd64.zip"
    );
    assert_eq!(
        abi_series("02.08.00").unwrap().hook_bundle_name(),
        "pandar-studio-hook-02.08.00-windows-amd64.zip"
    );
    assert_eq!(
        abi_series("02.08.01").unwrap().hook_bundle_name(),
        "pandar-studio-hook-02.08.01-windows-amd64.zip"
    );
    assert_eq!(
        abi_series("02.08.02").unwrap().hook_bundle_name(),
        "pandar-studio-hook-02.08.02-windows-amd64.zip"
    );
}

#[test]
fn rejects_unknown_or_duplicate_abi_series() {
    assert!(catalog().abi_series("02.09.00").is_err());
    let duplicate = ABI_SERIES_MANIFEST.replace("\"02.08.00\"", "\"02.07.01\"");
    assert!(StudioAbiSeriesCatalog::parse(&duplicate).is_err());
}
