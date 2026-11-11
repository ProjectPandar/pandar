use super::*;

#[test]
fn studio_print_metadata_rejects_unknown_or_non_finite_fields() {
    let unknown = r#"{"version":"1","task_name":"x","project_name":"x","preset_name":"x","nozzle_mapping":[],"ams_mapping":[],"ams_mapping2":[],"ams_mapping_info":[],"nozzles_info":[],"connection_type":"cloud","comments":"","origin_profile_id":0,"stl_design_id":0,"origin_model_id":"","print_type":"from_normal","submitted_device_name":"","task_bed_leveling":false,"task_flow_cali":false,"task_vibration_cali":false,"task_layer_inspect":false,"task_record_timelapse":false,"task_timelapse_use_internal":false,"task_use_ams":false,"task_bed_type":"auto","auto_bed_leveling":0,"auto_flow_cali":0,"auto_offset_cali":0,"extruder_cali_manual_mode":-1,"try_emmc_print":false,"svc_context":"","slicer_uid":"","password":"secret"}"#;
    assert!(serde_json::from_str::<StudioPrintMetadata>(unknown).is_err());
    assert!(StudioFiniteF64::try_from(f64::NAN).is_err());
    assert!(StudioFiniteF64::try_from(f64::INFINITY).is_err());
}

#[test]
fn studio_ams_mapping_info_accepts_pinned_unmatched_entry_shape() {
    let entry: StudioAmsMappingInfo =
        serde_json::from_str(r#"{"ams":-1,"targetColor":"","filamentId":"","filamentType":""}"#)
            .unwrap();

    assert_eq!(entry.nozzle_id, None);
    assert_eq!(entry.source_color, None);
}
