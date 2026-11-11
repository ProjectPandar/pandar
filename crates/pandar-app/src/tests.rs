use super::*;

#[test]
fn parses_agent_subcommand_with_agent_options() {
    let agent_id = "00000000-0000-4000-8000-000000000001";
    let tenant_id = "00000000-0000-4000-8000-000000000002";

    let cli = Cli::parse_from([
        "pandar",
        "agent",
        "--hub-grpc-url",
        "http://hub.internal:50051",
        "--agent-name",
        "garage",
        "--agent-id",
        agent_id,
        "--tenant-id",
        tenant_id,
        "--agent-credential",
        "pandar_ac_test",
    ]);

    let Command::Agent(config) = cli.command else {
        panic!("expected agent subcommand");
    };
    assert_eq!(config.hub_grpc_url, "http://hub.internal:50051");
    assert_eq!(config.agent_name, "garage");
    assert_eq!(config.agent_id, agent_id);
    assert_eq!(config.tenant_id, tenant_id);
    assert_eq!(config.agent_credential, "pandar_ac_test");
}

#[test]
fn defaults_install_network_plugin_files_to_release_artifacts_in_current_directory() {
    let cli = Cli::try_parse_from(["pandar", "install-network-plugin"])
        .expect("parse install-network-plugin without explicit release files");

    let Command::InstallNetworkPlugin {
        plugin_file,
        source_file,
        data_dir,
    } = cli.command
    else {
        panic!("expected install-network-plugin subcommand");
    };
    let expected_plugin_file = if cfg!(target_os = "windows") {
        "pandar_network_plugin.dll"
    } else if cfg!(target_os = "macos") {
        "libpandar_network_plugin.dylib"
    } else {
        "libpandar_network_plugin.so"
    };
    let expected_source_file = if cfg!(target_os = "windows") {
        "pandar_bambu_source.dll"
    } else if cfg!(target_os = "macos") {
        "libpandar_bambu_source.dylib"
    } else {
        "libpandar_bambu_source.so"
    };
    assert_eq!(plugin_file, PathBuf::from(expected_plugin_file));
    assert_eq!(source_file, PathBuf::from(expected_source_file));
    assert_eq!(data_dir, None);
}
