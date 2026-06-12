#![cfg(target_os = "linux")]

use rollshot_capture::{CaptureBackend, FrameStream};

#[test]
#[ignore = "requires installed Rollshot desktop entry and live KDE Wayland session"]
fn captures_linux_kwin_frames() {
    if std::env::var("ROLLSHOT_REAL_KWIN_CAPTURE").as_deref() != Ok("1") {
        eprintln!("set ROLLSHOT_REAL_KWIN_CAPTURE=1 to run real KWin capture");
        return;
    }

    let one_shot = rollshot_capture::OneShotBackendKind::LinuxKwin
        .capture_once(false)
        .expect("capture active KWin output");
    let output = one_shot
        .target_display()
        .output_name
        .clone()
        .expect("output name");

    let mut backend = rollshot_capture::LinuxKwinBackend::new(
        rollshot_capture::linux::kwin_screencast::RealKwinScreencastClient::new(),
        None,
    );
    let mut options = rollshot_capture::CaptureOptions::default();
    options.target_output_name = Some(output);
    let mut stream = backend.start(options).expect("start KWin stream");
    let frame = stream.next_frame().expect("first KWin frame");
    assert_eq!(frame.metadata.backend, "linux-kwin");
}
