use std::ffi::CStr;

use pandar_network_plugin::{STUDIO_ABI_SERIES, pandar_plugin_network_agent_version};

#[test]
fn network_agent_version_matches_selected_studio_abi_series() {
    let version = unsafe { CStr::from_ptr(pandar_plugin_network_agent_version()) }
        .to_str()
        .unwrap();

    let abi_series = pandar_studio_profile::abi_series(STUDIO_ABI_SERIES).unwrap();
    assert_eq!(version, abi_series.reported_network_agent_version);
}
