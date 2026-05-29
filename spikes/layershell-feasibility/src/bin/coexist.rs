#[path = "../overlay_app.rs"]
mod overlay_app;

use tao::event_loop::{ControlFlow, EventLoop};
use tao::platform::unix::WindowExtUnix;
use tao::window::WindowBuilder;
use wry::{WebViewBuilder, WebViewBuilderExtUnix};

fn main() -> wry::Result<()> {
    // 1. Spawn the layer-shell overlay on its OWN thread
    let (_tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("overlay".into())
        .spawn(move || {
            let _ = overlay_app::run(iced_layershell::settings::StartMode::Active, rx);
        })
        .expect("spawn overlay thread");

    // 2. Main thread brings up gtk + webkit2gtk (Tauri's footprint) via wry/tao.
    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("coexist host (Tauri-class webkit)")
        .build(&event_loop)
        .unwrap();

    let gtk_window = window.gtk_window();
    let _webview = WebViewBuilder::new()
        .with_html("<h1>webkit host alive</h1>")
        .build_gtk(gtk_window)?;

    event_loop.run(move |_event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
    });
}
