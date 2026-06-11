#![cfg(target_os = "linux")]

use std::path::PathBuf;

use rollshot_capture::{CaptureBackend, CaptureOptions, LinuxPortalBackend, RegionMode};

#[test]
#[ignore = "requires live Linux Wayland desktop and human portal picker interaction"]
fn captures_linux_portal_frames() {
    if std::env::var("ROLLSHOT_REAL_CAPTURE").as_deref() != Ok("1") {
        eprintln!("set ROLLSHOT_REAL_CAPTURE=1 to run real Linux portal capture");
        return;
    }
    assert_eq!(
        std::env::var("XDG_SESSION_TYPE").as_deref(),
        Ok("wayland"),
        "linux portal smoke test requires XDG_SESSION_TYPE=wayland"
    );

    let mut backend = LinuxPortalBackend::new();
    let mut stream = backend
        .start(CaptureOptions {
            region: RegionMode::PortalPicker,
            fps: 5,
            show_cursor: false,
            prefer_portal_region: true,
            target_display_id: None,
        })
        .expect("start linux portal capture");

    let mut first = None;
    for _ in 0..3 {
        let frame = stream.next_frame().expect("next portal frame");
        assert!(frame.image.width() > 0);
        assert!(frame.image.height() > 0);
        assert_eq!(frame.metadata.backend, "linux-portal");
        first.get_or_insert(frame);
    }

    let artifact = PathBuf::from("target/test-artifacts/linux_portal_first_frame.png");
    std::fs::create_dir_all(artifact.parent().unwrap()).unwrap();
    first.unwrap().image.save(&artifact).unwrap();
}
