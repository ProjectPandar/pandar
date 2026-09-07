use pandar_network_plugin::studio_status::project_hub_printers;

#[path = "studio_status/ams_materials.rs"]
mod ams_materials;

fn telemetry(printer: &str) -> String {
    let fields = serde_json::from_str::<serde_json::Value>(printer).unwrap();
    let mut device = serde_json::json!({
        "dev_id": "studio-serial-1",
        "pandar_printer_id": "printer-1",
        "dev_online": true,
        "online": true,
        "firmware": null
    });
    device
        .as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    let body = serde_json::json!({"message": "success", "devices": [device]}).to_string();
    let projection = project_hub_printers(&body).unwrap();
    projection.printers()[0]
        .status_report()
        .strip_prefix(r#"{"print":"#)
        .and_then(|status| status.strip_suffix('}'))
        .expect("Studio push status envelope")
        .to_owned()
}

fn telemetry_json(printer: &str) -> serde_json::Value {
    serde_json::from_str(&telemetry(printer)).unwrap()
}

#[test]
fn studio_status_emits_native_print_error_and_job_id() {
    let telemetry = telemetry_json(r#"{"print_error":83918929,"job_id":"job-7"}"#);

    assert_eq!(telemetry["print_error"], serde_json::json!(83_918_929));
    assert_eq!(telemetry["job_id"], serde_json::json!("job-7"));
}

#[test]
fn studio_status_preserves_explicit_clear_and_empty_job_id() {
    let telemetry = telemetry_json(r#"{"print_error":0,"job_id":""}"#);

    assert_eq!(telemetry["print_error"], serde_json::json!(0));
    assert_eq!(telemetry["job_id"], serde_json::json!(""));
}

#[test]
fn studio_status_omits_unknown_native_error_fields() {
    let telemetry = telemetry_json("{}");

    assert!(telemetry.get("print_error").is_none());
    assert!(telemetry.get("job_id").is_none());
}

#[test]
fn studio_status_masks_unsupported_fun_bits_and_preserves_supported_and_unknown_bits() {
    let telemetry = telemetry_json(r#"{"fun":"D0027D41100037C0"}"#);

    assert_eq!(telemetry["fun"], serde_json::json!("8000004100000000"));
}

#[test]
fn studio_status_exposes_nozzle_rack_only_with_projected_rack_telemetry() {
    let without_rack = telemetry_json(r#"{"fun":"1000000000000000"}"#);
    assert_eq!(without_rack["fun"], serde_json::json!("0"));

    let with_rack = telemetry_json(
        r#"{"fun":"1000000000000000","nozzle_temperatures":[{"label":"R","current_celsius":"27","target_celsius":"220","snow":16,"hnow":16}],"nozzle_system":{"nozzle":{"exist":65536,"state":0,"src_id":16,"tar_id":17,"info":[{"id":16,"diameter":0.4,"type":"XS01","stat":0}]},"holder":{"stat":0,"pos":2,"info":0}}}"#,
    );
    assert_eq!(with_rack["fun"], serde_json::json!("1000000000000000"));
    assert_eq!(with_rack["device"]["nozzle"]["info"][0]["id"], 16);
    assert_eq!(with_rack["device"]["holder"]["pos"], 2);
    assert_eq!(with_rack["device"]["extruder"]["info"][0]["snow"], 16);
    assert_eq!(with_rack["device"]["extruder"]["info"][0]["hnow"], 16);

    let without_routing = telemetry_json(
        r#"{"fun":"1000000000000000","nozzle_system":{"nozzle":{"exist":65536,"state":0,"info":[{"id":16,"diameter":0.4,"type":"XS01","stat":0}]}}}"#,
    );
    assert_eq!(
        without_routing["device"]["extruder"]["info"][0]["hnow"],
        65535
    );
}

#[test]
fn studio_status_hides_external_change_assist_while_print_field_is_unsupported() {
    let telemetry = telemetry_json(r#"{"fun":"1000000000000"}"#);

    assert_eq!(telemetry["fun"], serde_json::json!("0"));
}

#[test]
fn studio_status_masks_unsupported_cfg_bits_without_rewriting_observed_storage() {
    let telemetry = telemetry_json(r#"{"materials":{"cfg":"800000C000000001","ams_units":[]}}"#);

    assert_eq!(telemetry["cfg"], serde_json::json!("8000000000000001"));
}

#[test]
fn studio_status_keeps_raw_camera_bits_masked_for_hub_mediated_camera() {
    let telemetry =
        telemetry_json(r#"{"fun":"4100000002","materials":{"cfg":"4C000000001","ams_units":[]}}"#);

    assert_eq!(telemetry["fun"], serde_json::json!("4100000000"));
    assert_eq!(telemetry["cfg"], serde_json::json!("1"));
}

#[test]
fn studio_status_reports_sdcard_only_for_aux_normal_state() {
    for (printer, expected) in [
        (r#"{"materials":{"aux":"00001000"}}"#, true),
        (r#"{"materials":{"aux":"00000000"}}"#, false),
        (r#"{"materials":{"aux":"00002000"}}"#, false),
        (r#"{"materials":{"aux":"00003000"}}"#, false),
        (r#"{"materials":{"aux":"not-hex"}}"#, false),
        (r#"{"materials":{}}"#, false),
        (r#"{}"#, false),
    ] {
        let telemetry = telemetry_json(printer);

        assert_eq!(telemetry["sdcard"], serde_json::json!(expected));
    }
}

#[test]
fn studio_status_preserves_chamber_current_and_target_in_v1_and_v2_shapes() {
    for (printer, current, target, packed, support) in [
        (r#"{}"#, 0, 0, 0, false),
        (
            r#"{"chamber_target_temperature_celsius":"45"}"#,
            0,
            45,
            2_949_120,
            false,
        ),
        (r#"{"chamber_temperature_celsius":"32"}"#, 32, 0, 32, false),
        (
            r#"{"chamber_temperature_celsius":"32","chamber_target_temperature_celsius":"45"}"#,
            32,
            45,
            2_949_152,
            true,
        ),
    ] {
        let telemetry = telemetry_json(printer);

        assert_eq!(telemetry["chamber_temper"], serde_json::json!(current));
        assert_eq!(telemetry["ctt"], serde_json::json!(target));
        assert_eq!(telemetry["device"]["ctc"]["info"]["temp"], packed);
        assert_eq!(telemetry["support_chamber"], support);
        assert_eq!(telemetry["support_chamber_temp_display"], support);
    }
}

#[test]
fn studio_status_defaults_missing_or_null_fun_without_discarding_telemetry() {
    for fun in ["", r#","fun":null"#] {
        let telemetry = telemetry_json(&format!(
            r#"{{"gcode_state":"RUNNING","mc_percent":37,"bed_temperature_celsius":"60","hms":[{{"attr":134152704,"code":32785}}],"materials":{{"ams_units":[{{"unit_id":"0","trays":[{{"tray_id":"0","type":"PETG-CF"}}]}}]}}{fun}}}"#
        ));

        assert_eq!(telemetry["fun"], serde_json::json!("0"));
        assert_eq!(telemetry["gcode_state"], serde_json::json!("RUNNING"));
        assert_eq!(telemetry["mc_percent"], serde_json::json!(37));
        assert_eq!(telemetry["bed_temper"], serde_json::json!(60));
        assert_eq!(telemetry["hms"][0]["attr"], serde_json::json!(134_152_704));
        assert_eq!(telemetry["hms"][0]["code"], serde_json::json!(32_785));
        assert_eq!(
            telemetry["ams"]["ams"][0]["tray"][0]["tray_type"],
            serde_json::json!("PETG-CF")
        );
    }
}

#[test]
fn printer_telemetry_omits_unknown_model_and_state() {
    let body = telemetry("{}");
    let json: serde_json::Value = serde_json::from_str(&body).unwrap();

    assert!(json.get("gcode_state").is_none());
    assert!(body.contains(r#""mc_percent":0"#));
    assert!(body.contains(r#""mc_remaining_time":0"#));
    assert!(body.contains(r#""layer_num":0"#));
    assert!(body.contains(r#""total_layer_num":0"#));
    assert!(body.contains(r#""project_id":"0""#));
    assert!(body.contains(r#""profile_id":"0""#));
    assert!(body.contains(r#""subtask_id":"0""#));
    assert!(body.contains(r#""hms":[]"#));
    assert!(json.get("printer_type").is_none());
    assert!(body.contains(r#""support_chamber":false"#));
    assert!(body.contains(r#""support_chamber_temp_display":false"#));
    assert!(json.get("cfg").is_none());
    assert!(json.get("aux").is_none());
    assert!(json.get("stat").is_none());
    assert!(body.contains(r#""bed_temper":0"#));
    assert!(body.contains(
        r#""nozzle":{"exist":1,"state":0,"info":[{"id":0,"diameter":0.4,"type":"XS01","stat":0}]}"#
    ));
    assert!(body.contains(r#""extruder":{"state":1,"info":[{"id":0,"filam_bak":[],"info":8,"temp":0,"spre":65535,"snow":65535,"star":65535,"stat":0,"hnow":0}]}"#));
    assert!(body.contains(r#","ams":{"ams":[]}"#));
}

#[test]
fn printer_telemetry_maps_dual_nozzle_temperatures_and_active_tool() {
    let body = telemetry(
        r#"{"dev_model_name":"N6","nozzle_temperatures":[{"label":"L","current_celsius":"28","target_celsius":"220","diameter_mm":"0.4","nozzle_type":"XH05"},{"label":"R","current_celsius":"27","target_celsius":"215","diameter_mm":"0.6","nozzle_type":"XS01"}],"active_nozzle":"L","bed_temperature_celsius":"60","bed_target_temperature_celsius":"65","chamber_temperature_celsius":"32","chamber_light_on":true}"#,
    );

    assert!(body.contains(r#""printer_type":"N6""#));
    assert!(body.contains(r#""nozzle_temper":27"#));
    assert!(body.contains(r#""nozzle_target_temper":215"#));
    assert!(body.contains(r#""nozzle_temper2":28"#));
    assert!(body.contains(r#""nozzle_target_temper2":220"#));
    assert!(body.contains(r#""nozzle_type":"XS01""#));
    assert!(body.contains(r#""nozzle_diameter":0.6"#));
    assert!(body.contains(r#""nozzle_type2":"XH05""#));
    assert!(body.contains(r#""nozzle_diameter2":0.4"#));
    assert!(body.contains(r#""bed_temp":4259900"#));
    assert!(body.contains(r#""ctc":{"state":1,"info":{"temp":32}}"#));
    assert!(body.contains(r#""nozzle":{"exist":3"#));
    assert!(body.contains(r#"{"id":0,"diameter":0.6,"type":"XS01","stat":0}"#));
    assert!(body.contains(r#"{"id":1,"diameter":0.4,"type":"XH05","stat":0}"#));
    assert!(body.contains(r#"{"id":0,"diameter":0.6,"type":"XS01","stat":0},{"id":1,"diameter":0.4,"type":"XH05","stat":0}"#));
    assert!(body.contains(r#""extruder":{"state":18"#));
    assert!(body.contains(r#"{"id":1,"filam_bak":[],"info":8,"temp":14417948,"spre":65535,"snow":65535,"star":65535,"stat":0,"hnow":1}"#));
    assert!(body.contains(r#"{"id":0,"filam_bak":[],"info":8,"temp":14090267,"spre":65535,"snow":65535,"star":65535,"stat":0,"hnow":0},{"id":1,"filam_bak":[],"info":8,"temp":14417948,"spre":65535,"snow":65535,"star":65535,"stat":0,"hnow":1}"#));
    assert!(body.contains(r#""lights_report":[{"node":"chamber_light","mode":"on"}]"#));
}

#[test]
fn printer_telemetry_maps_live_print_progress_and_hms() {
    let body = telemetry(
        r#"{"gcode_state":"RUNNING","mc_percent":37,"mc_remaining_time":52,"layer_num":12,"total_layer_num":120,"task_id":"task-42","subtask_id":"subtask-42","gcode_file":"drawer-organizer.gcode","subtask_name":"drawer-organizer","hms":[{"attr":134152704,"code":32785}]}"#,
    );

    assert!(body.contains(r#""gcode_state":"RUNNING""#));
    assert!(body.contains(r#""mc_percent":37"#));
    assert!(body.contains(r#""mc_remaining_time":52"#));
    assert!(body.contains(r#""layer_num":12"#));
    assert!(body.contains(r#""total_layer_num":120"#));
    assert!(body.contains(r#""task_id":"task-42""#));
    assert!(body.contains(r#""project_id":"0""#));
    assert!(body.contains(r#""profile_id":"0""#));
    assert!(body.contains(r#""subtask_id":"subtask-42""#));
    assert!(body.contains(r#""gcode_file":"drawer-organizer.gcode""#));
    assert!(body.contains(r#""subtask_name":"drawer-organizer""#));
    assert!(body.contains(r#""hms":[{"attr":134152704,"code":32785}]"#));
}

#[test]
fn nullable_chamber_light_does_not_discard_live_print_status() {
    let body = telemetry(
        r#"{"gcode_state":"RUNNING","mc_percent":37,"hms":[{"attr":134152704,"code":32785}],"chamber_light_on":null}"#,
    );

    assert!(body.contains(r#""gcode_state":"RUNNING""#));
    assert!(body.contains(r#""mc_percent":37"#));
    assert!(body.contains(r#""hms":[{"attr":134152704,"code":32785}]"#));
    assert!(body.contains(r#""lights_report":[]"#));
}

#[test]
fn printer_telemetry_uses_present_state_without_inventing_idle() {
    let telemetry = telemetry_json(r#"{"state":"PAUSE"}"#);

    assert_eq!(telemetry["gcode_state"], serde_json::json!("PAUSE"));
}

#[test]
fn explicit_chamber_light_off_is_reported_as_off() {
    let body = telemetry(r#"{"chamber_light_on":false}"#);

    assert!(
        body.contains(r#""lights_report":[{"node":"chamber_light","mode":"off"}]"#),
        "{body}"
    );
}
