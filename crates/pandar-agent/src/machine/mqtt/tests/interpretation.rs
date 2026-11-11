use super::*;

#[test]
fn interpretation_preserves_snapshot_when_materials_fail_to_decode() {
    let interpreted = interpret_report(
        &endpoint(),
        serde_json::json!({
            "print": {
                "gcode_state": "RUNNING",
                "ams": { "ams": "invalid" }
            }
        }),
    );

    assert_eq!(
        interpreted.snapshot.unwrap().state.as_deref(),
        Some("RUNNING")
    );
    assert!(interpreted.materials.is_none());
    assert_eq!(interpreted.diagnostics.len(), 1);
    assert_eq!(
        interpreted.diagnostics[0].section,
        report::MachineReportSection::Materials
    );
}
