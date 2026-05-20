#![cfg(all(target_os = "macos", feature = "macos-sck"))]

use std::path::Path;

use rollshot_capture::{
    CaptureBackend, CaptureOptions, MacosScreenCaptureKitBackend, PixelFormat, Region, RegionMode,
};

#[test]
#[ignore = "requires macOS Screen Recording permission and an interactive desktop session"]
fn macos_sck_receives_frames() {
    if std::env::var("ROLLSHOT_REAL_CAPTURE").ok().as_deref() != Some("1") {
        eprintln!("set ROLLSHOT_REAL_CAPTURE=1 to run the real macOS capture smoke test");
        return;
    }

    let mut backend = MacosScreenCaptureKitBackend::new();
    let options = CaptureOptions {
        region: RegionMode::Manual(Region {
            x: 0,
            y: 0,
            width: 320,
            height: 240,
        }),
        fps: 5,
        show_cursor: false,
        prefer_portal_region: false,
    };

    let mut stream = backend.start(options).expect("start macOS capture");
    let mut first_frame = None;

    for _ in 0..3 {
        let frame = stream.next_frame().expect("next macOS capture frame");
        assert!(frame.image.width() > 0);
        assert!(frame.image.height() > 0);
        assert_eq!(frame.metadata.backend, "macos-sck");
        assert_eq!(frame.metadata.pixel_format, Some(PixelFormat::Bgra));

        if first_frame.is_none() {
            first_frame = Some(frame);
        }
    }

    let frame = first_frame.expect("first frame captured");
    let artifact_dir = Path::new("target/test-artifacts");
    std::fs::create_dir_all(artifact_dir).expect("create artifact dir");
    frame
        .image
        .save(artifact_dir.join("macos_sck_first_frame.png"))
        .expect("save first frame artifact");
}
