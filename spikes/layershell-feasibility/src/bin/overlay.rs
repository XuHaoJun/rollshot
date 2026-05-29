#[path = "../overlay_app.rs"]
mod overlay_app;

fn main() -> Result<(), iced_layershell::Error> {
    overlay_app::run(iced_layershell::settings::StartMode::Active)
}
