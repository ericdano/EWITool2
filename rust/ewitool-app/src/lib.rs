//! EWITool2 GUI. `main.rs` only starts the window; everything else lives here
//! so UI tests can drive the app headless.

pub mod app;
mod audio;
pub mod editor;
pub mod midi;
#[cfg(target_os = "macos")]
mod native_menu;
mod prefs;
pub mod theme;

pub use app::App;
