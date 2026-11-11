use super::*;
use std::{fs, io::Write};

fn temp_file(dir: &tempfile::TempDir, name: &str) -> PathBuf {
    let path = dir.path().join(name);
    let mut file = fs::File::create(&path).unwrap();
    writeln!(file, "placeholder").unwrap();
    path
}

fn valid_args(dir: &tempfile::TempDir) -> Args {
    Args {
        studio_path: temp_file(dir, "BambuStudio"),
        plugin_artifact: temp_file(dir, "libpandar_network_plugin.so"),
        hub_url: "https://hub.example".to_owned(),
        frontend_url: "https://web.example".to_owned(),
        os: TargetOs::Linux,
        arch: "x86_64".to_owned(),
        studio_version: "1.10.2".to_owned(),
        test_date: "2026-06-24".to_owned(),
        pandar_commit: "abcdef123456".to_owned(),
    }
}

fn valid_cli_args(dir: &tempfile::TempDir) -> Vec<String> {
    let args = valid_args(dir);
    vec![
        "--preflight".to_owned(),
        "--studio-path".to_owned(),
        args.studio_path.display().to_string(),
        "--plugin-artifact".to_owned(),
        args.plugin_artifact.display().to_string(),
        "--hub-url".to_owned(),
        args.hub_url,
        "--frontend-url".to_owned(),
        args.frontend_url,
        "--os".to_owned(),
        args.os.as_str().to_owned(),
        "--arch".to_owned(),
        args.arch,
        "--studio-version".to_owned(),
        args.studio_version,
        "--test-date".to_owned(),
        args.test_date,
        "--pandar-commit".to_owned(),
        args.pandar_commit,
    ]
}

#[test]
fn url_credentials_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut args = valid_args(&dir);
    args.hub_url = "https://user:pass@hub.example".to_owned();

    let error = validate(&args).unwrap_err();

    assert!(error.contains("hub-url must not include credentials"));
}

#[test]
fn valid_preflight_output_includes_explicit_test_date_in_evidence_row() {
    let dir = tempfile::tempdir().unwrap();
    let mut raw_args = valid_cli_args(&dir);
    let date_position = raw_args
        .iter()
        .position(|arg| arg == "--test-date")
        .unwrap()
        + 1;
    raw_args[date_position] = "2030-01-02".to_owned();

    let args = parse_args(raw_args).unwrap();
    validate(&args).unwrap();
    let output = render_output(&args);

    assert!(output.lines().last().unwrap().contains("2030-01-02"));
}

#[test]
fn plugin_filename_mismatch_reports_filenames_without_full_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut args = valid_args(&dir);
    args.plugin_artifact = temp_file(&dir, "pandar_network_plugin.dll");

    let error = validate(&args).unwrap_err();

    assert!(error.contains("pandar_network_plugin.dll"));
    assert!(error.contains("expected libpandar_network_plugin.so"));
    assert!(!error.contains(dir.path().to_str().unwrap()));
}
