//! CAD-viser: en enkel DWG/DXF-viser for Android og desktop, bygget på CADCraft.

mod about;
mod app;
mod document;
mod platform;
mod view;

pub use app::ViewerApp;

pub const APP_NAME: &str = "CAD-viser";

/// Start appen på desktop. `open`: en fil som skal åpnes med en gang.
#[cfg(not(target_os = "android"))]
pub fn run_desktop(open: Option<std::path::PathBuf>) -> eframe::Result {
    let open = open.map(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        (p, name)
    });
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_title(APP_NAME).with_inner_size([1100.0, 760.0]).with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(APP_NAME, options, Box::new(move |cc| Ok(Box::new(ViewerApp::new(cc, open)))))
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("cadviewer"));
    if let Some(dir) = app.internal_data_path()
        && let Ok(mut d) = platform::DATA_DIR.lock()
    {
        *d = Some(dir);
    }
    log::info!("android_main startet");
    platform::set_activity(app.vm_as_ptr(), app.activity_as_ptr());
    let options = eframe::NativeOptions { android_app: Some(app), ..Default::default() };
    if let Err(e) = eframe::run_native(APP_NAME, options, Box::new(|cc| Ok(Box::new(ViewerApp::new(cc, None))))) {
        log::error!("Appen stoppet: {e}");
    }
}
