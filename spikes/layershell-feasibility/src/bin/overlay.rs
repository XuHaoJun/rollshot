#[path = "../overlay_app.rs"]
mod overlay_app;

use std::sync::mpsc;
use std::thread;

fn main() -> Result<(), iced_layershell::Error> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let colors: [(u8, u8, u8); 3] = [(255, 0, 0), (0, 255, 0), (0, 0, 255)];
        let mut i = 0u32;
        loop {
            let (r, g, b) = colors[i as usize % 3];
            let mut pixels = Vec::with_capacity(200 * 200 * 4);
            for _ in 0..(200 * 200) {
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
            let handle = iced::widget::image::Handle::from_rgba(200, 200, pixels);
            if tx.send(handle).is_err() {
                break;
            }
            i += 1;
            thread::sleep(std::time::Duration::from_millis(100));
        }
    });

    overlay_app::run(iced_layershell::settings::StartMode::Active, rx)
}
