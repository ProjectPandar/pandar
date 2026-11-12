use std::process::Command;

/// Runs every dry-run scenario through the real CLI so Hub API drift fails
/// the workspace suite instead of silently breaking the soak harness.
#[test]
fn dry_run_scenarios_pass() {
    let output = Command::new(env!("CARGO_BIN_EXE_pandar-scaled-artifact-smoke"))
        .args(["--dry-run", "--iterations", "1", "--concurrency", "2"])
        // Throwaway all-zero AES-256 key for the disposable dry-run database.
        .env(
            "PANDAR_PRINTER_ACCESS_CODE_KEY",
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        )
        .output()
        .expect("run scaled artifact smoke");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "dry run failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    for scenario in ["artifact", "fanout", "restart", "storage", "terminal"] {
        assert!(
            stdout.contains(&format!("PASS scenario={scenario} iteration=1")),
            "scenario {scenario} did not report a pass\nstdout:\n{stdout}"
        );
    }
}
