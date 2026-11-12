use anyhow::Result;
use rustcord::app::RustcordApp;
use rustcord::settings::Settings;

fn main() -> Result<()> {
    // Capture logs both on stderr and in the in-memory ring used by the
    // Diagnostics page.
    rustcord::logbuf::install("info");

    let mut settings = Settings::load_or_default();
    settings.normalize();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Rustcord"),
        ..Default::default()
    };

    eframe::run_native(
        "Rustcord",
        native_options,
        Box::new(move |cc| Ok(Box::new(RustcordApp::new(cc, settings)))),
    )
    .map_err(|err| anyhow::anyhow!("eframe failed: {err}"))?;

    Ok(())
}
