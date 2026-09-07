use super::{telemetry, telemetry_json};

#[test]
fn printer_telemetry_maps_ams_and_external_materials() {
    let body = telemetry(
        r##"{"materials":{"ams_units":[{"unit_id":"0","humidity":25,"humidity_level":3,"temperature_celsius":28.5,"toolhead":"R","trays":[{"tray_id":"0","global_tray_id":0,"type":"PETG-CF","filament_id":"GFG50","color":"000000FF","remaining_estimate":"-1"},{"tray_id":"1","global_tray_id":1,"type":"PLA","filament_id":"GFA00","color":"C12E1FFF","remaining_estimate":"100"}]},{"unit_id":"1","humidity":28,"humidity_level":3,"temperature_celsius":28.1,"toolhead":"L","trays":[{"tray_id":"0","global_tray_id":4,"type":"PLA","filament_id":"GFA00","color":"000000FF","remaining_estimate":"55"}]}],"external_spools":[{"external_id":"254","tray_id":"0","type":"PETG","filament_id":"GFG00","color":"11223344","toolhead":"L"},{"external_id":"255","tray_id":"1","type":"PLA","filament_id":"GFL99","color":"46A8F9FF","toolhead":"R"}],"active_tray":{"kind":"ams","ams_id":"0","tray_id":"1","global_tray_id":1}}}"##,
    );

    assert!(body.contains(r#""ams_exist_bits":"3""#));
    assert!(body.contains(r#""tray_exist_bits":"13""#));
    assert!(body.contains(r#""tray_now":"1""#));
    assert!(body.contains(r#""id":"0","info":"1""#));
    assert!(body.contains(r#""id":"1","info":"101""#));
    assert!(body.contains(r#""humidity":"3""#));
    assert!(body.contains(r#""humidity_raw":"25""#));
    assert!(body.contains(r#""temp":"28.5""#));
    assert!(body.contains(r#""tray_info_idx":"GFG50""#));
    assert!(body.contains(r#""tray_type":"PETG-CF""#));
    assert!(body.contains(r#""remain":-1"#));
    assert!(body.contains(r#""vir_slot":[{"id":"254""#));
    assert!(body.contains(r#""id":"255""#));
    assert!(body.contains(r#""tray_type":"PETG""#));
    assert!(!body.contains(r#""aux":"#));
}

#[test]
fn printer_telemetry_projects_a2l_mixed_ams_lite_routing() {
    let telemetry = telemetry_json(
        r#"{"materials":{"ams_units":[{"unit_id":"0","unit_kind":"ams_lite_mixed","toolhead":"R","trays":[{"tray_id":"0","global_tray_id":24,"exists":true,"type":"PLA"},{"tray_id":"1","global_tray_id":25,"exists":false},{"tray_id":"2","global_tray_id":26,"exists":false},{"tray_id":"3","global_tray_id":27,"exists":false}]}],"external_spools":[],"active_tray":{"kind":"ams","global_tray_id":24}}}"#,
    );

    assert_eq!(
        telemetry["ams"]["ams_exist_bits"],
        serde_json::json!("1000")
    );
    assert_eq!(
        telemetry["ams"]["tray_exist_bits"],
        serde_json::json!("1000000")
    );
    assert_eq!(telemetry["ams"]["tray_now"], serde_json::json!("24"));
    assert_eq!(telemetry["ams"]["ams"][0]["info"], serde_json::json!("5"));
}

#[test]
fn printer_telemetry_preserves_filament_switch_aux_and_routes() {
    let telemetry = telemetry_json(
        r#"{"materials":{"cfg":"8000000000000001","aux":"A4003001","stat":"1000000001","filament_switch_installed":true,"ams_units":[{"unit_id":"0","info":"00000E00","toolhead":"LR","trays":[]},{"unit_id":"1","info":"01000E00","toolhead":"LR","trays":[]}]}}"#,
    );

    assert_eq!(telemetry["cfg"], serde_json::json!("8000000000000001"));
    assert_eq!(telemetry["aux"], serde_json::json!("A4003001"));
    assert_eq!(telemetry["stat"], serde_json::json!("1000000001"));
    assert_eq!(telemetry["ams"]["ams_exist_bits"], serde_json::json!("3"));
    assert_eq!(telemetry["ams"]["ams"][0]["id"], serde_json::json!("0"));
    assert_eq!(
        telemetry["ams"]["ams"][0]["info"],
        serde_json::json!("00000E00")
    );
    assert_eq!(telemetry["ams"]["ams"][1]["id"], serde_json::json!("1"));
    assert_eq!(
        telemetry["ams"]["ams"][1]["info"],
        serde_json::json!("01000E00")
    );
}

#[test]
fn printer_telemetry_keeps_legacy_lr_projection_when_switch_is_absent() {
    let telemetry = telemetry_json(
        r#"{"materials":{"aux":"84001000","filament_switch_installed":false,"ams_units":[{"unit_id":"0","info":"00000E00","toolhead":"R","trays":[]},{"unit_id":"1","info":"01000E00","toolhead":"L","trays":[]}]}}"#,
    );

    assert_eq!(telemetry["aux"], serde_json::json!("84001000"));
    assert_eq!(telemetry["ams"]["ams"][0]["info"], serde_json::json!("1"));
    assert_eq!(telemetry["ams"]["ams"][1]["info"], serde_json::json!("101"));
}

#[test]
fn printer_telemetry_does_not_invent_aux_when_raw_state_is_missing() {
    let telemetry =
        telemetry_json(r#"{"materials":{"filament_switch_installed":true,"ams_units":[]}}"#);

    assert!(telemetry.get("aux").is_none());
}

#[test]
fn printer_telemetry_preserves_explicit_empty_studio_flags() {
    let telemetry = telemetry_json(
        r#"{"materials":{"cfg":"","aux":"","stat":"","filament_switch_installed":false,"ams_units":[]}}"#,
    );

    assert_eq!(telemetry["cfg"], serde_json::json!(""));
    assert_eq!(telemetry["aux"], serde_json::json!(""));
    assert_eq!(telemetry["stat"], serde_json::json!(""));
}

#[test]
fn printer_telemetry_synthesizes_switch_binding_for_units_without_raw_info() {
    let telemetry = telemetry_json(
        r#"{"materials":{"aux":"20000000","filament_switch_installed":true,"ams_units":[{"unit_id":"0","toolhead":"LR","trays":[{"tray_id":"0"}]},{"unit_id":"1","toolhead":"LR","trays":[{"tray_id":"0"}]}]}}"#,
    );

    assert_eq!(telemetry["ams"]["ams_exist_bits"], serde_json::json!("3"));
    assert_eq!(telemetry["ams"]["tray_exist_bits"], serde_json::json!("11"));
    assert_eq!(telemetry["ams"]["ams"][0]["id"], serde_json::json!("0"));
    assert_eq!(telemetry["ams"]["ams"][0]["info"], serde_json::json!("e01"));
    assert_eq!(telemetry["ams"]["ams"][1]["id"], serde_json::json!("1"));
    assert_eq!(telemetry["ams"]["ams"][1]["info"], serde_json::json!("e01"));
}

#[test]
fn printer_telemetry_falls_back_to_toolhead_binding_when_switch_info_is_unusable() {
    // Raw unit `info` nibbles: bits 8-11 bind extruders (0xE = both), bits 24-27
    // are the filament-switch input, which Studio only accepts as 0 or 1 — so
    // unit 4's input of 2 makes its raw route unusable despite a valid 0xE.
    let telemetry = telemetry_json(
        r#"{"materials":{"aux":"20000000","filament_switch_installed":true,"ams_units":[{"unit_id":"0","info":"00000E00","toolhead":"LR","trays":[{"tray_id":"0"}]},{"unit_id":"1","toolhead":"LR","trays":[{"tray_id":"0"}]},{"unit_id":"2","info":"00000000","toolhead":"LR","trays":[{"tray_id":"0"}]},{"unit_id":"3","info":"not-hex","toolhead":"LR","trays":[{"tray_id":"0"}]},{"unit_id":"4","info":"02000E00","toolhead":"LR","trays":[{"tray_id":"0"}]}]}}"#,
    );

    assert_eq!(telemetry["ams"]["ams_exist_bits"], serde_json::json!("1f"));
    assert_eq!(
        telemetry["ams"]["ams"][0]["info"],
        serde_json::json!("00000E00")
    );
    assert_eq!(telemetry["ams"]["ams"][1]["info"], serde_json::json!("e01"));
    assert_eq!(telemetry["ams"]["ams"][2]["info"], serde_json::json!("e01"));
    assert_eq!(telemetry["ams"]["ams"][3]["info"], serde_json::json!("e01"));
    assert_eq!(telemetry["ams"]["ams"][4]["info"], serde_json::json!("e01"));
}

#[test]
fn printer_telemetry_keeps_legacy_main_binding_for_lr_units_without_switch_flag() {
    let telemetry = telemetry_json(
        r#"{"materials":{"ams_units":[{"unit_id":"0","toolhead":"LR","trays":[]}]}}"#,
    );

    assert_eq!(telemetry["ams"]["ams"][0]["info"], serde_json::json!("1"));
}
