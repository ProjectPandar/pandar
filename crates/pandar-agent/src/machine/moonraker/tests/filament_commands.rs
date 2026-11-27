use pandar_protocol::agent::v1::{AmsLoadFilamentOperation, printer_operation};

use super::fake_moonraker::{API_KEY, FakeMoonraker};
use super::support::{Harness, link, operate};

/// Firmware without the Snapmaker filament extensions must reject the channel
/// operations before any G-code reaches the printer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn moonraker_filament_operations_require_the_firmware_commands() {
    let printer = FakeMoonraker::start_with_gcode_help(&["M106"]).await;
    let mut harness = Harness::start().await;
    harness.run("link", link(&printer.host, API_KEY)).await;
    harness.result("link").await;

    harness
        .run(
            "unload",
            operate(printer_operation::Operation::AmsLoadFilament(
                AmsLoadFilamentOperation {
                    ams_id: 0,
                    slot_id: 2,
                    ..Default::default()
                },
            )),
        )
        .await;
    let result = harness.result("unload").await;

    assert!(!result.success, "{result:?}");
    assert!(
        result
            .error
            .contains("firmware has no AUTO_FEEDING command"),
        "{}",
        result.error
    );
    assert!(
        printer.requests().await.is_empty(),
        "unload must not reach the printer"
    );
}
