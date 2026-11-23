#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // no console window on Windows

use eframe::egui;
use ewitool_app::app;

fn main() -> eframe::Result {
    // Windows release builds have no console of their own, so the
    // command-line options (--fetch, --midi-check, --smoke) would print
    // nothing. Borrow the console they were started from, if any.
    #[cfg(windows)]
    if std::env::args().len() > 1 {
        use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
        // SAFETY: no pointers involved; fails harmlessly without a console.
        unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
    }

    // Used by release checks to confirm an installed build starts.
    if std::env::args().any(|a| a == "--smoke") {
        println!("{} {} started", app::APP_NAME, app::APP_VERSION);
        return Ok(());
    }

    // Troubleshooting: list MIDI ports and look for the EWI without the GUI.
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--midi-check") {
        if let Err(e) = ewitool_app::midi::diagnose(args.get(i + 1).map(String::as_str)) {
            eprintln!("MIDI error: {e}");
        }
        return Ok(());
    }

    // Back up the EWI's patches without the GUI.
    if let Some(i) = args.iter().position(|a| a == "--fetch") {
        let (Some(port), Some(file)) = (args.get(i + 1), args.get(i + 2)) else {
            eprintln!("usage: ewitool2 --fetch <MIDI port> <file.syx>");
            std::process::exit(2);
        };
        if let Err(e) = ewitool_app::midi::fetch_to_file(port, std::path::Path::new(file)) {
            eprintln!("Fetch failed: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(app::APP_NAME)
        .with_inner_size([1280.0, 860.0])
        .with_min_inner_size([800.0, 500.0]);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/logo.png"))
    {
        viewport = viewport.with_icon(icon);
    }

    eframe::run_native(
        app::APP_NAME,
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| {
            #[allow(unused_mut)]
            let mut app = app::App::new();
            // A real macOS menu bar instead of menus inside the window.
            #[cfg(target_os = "macos")]
            app.install_native_menu(&cc.egui_ctx);
            let _ = cc;
            Ok(Box::new(app))
        }),
    )
}
