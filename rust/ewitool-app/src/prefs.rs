//! User preferences (port of `UserPrefs.java`), stored as `key=value` lines
//! in the OS config folder, e.g. `~/Library/Application Support/...` on
//! macOS or `%APPDATA%\...` on Windows.

use std::io;

use eframe::egui::{Color32, ThemePreference};
use std::path::{Path, PathBuf};

use crate::theme::{Look, MAX_CORNERS};

/// Text size choices offered in Preferences, in percent.
pub const TEXT_SIZES: [u32; 4] = [90, 100, 115, 130];

#[derive(Debug, Default)]
pub struct Prefs {
    look: Look,
    library: Option<PathBuf>,
    midi_in: Option<String>,
    midi_out: Option<String>,
    theme: ThemePreference,
    /// Patch Editor size in percent; `None` means the default.
    editor_size: Option<u32>,
    file: Option<PathBuf>,
}

/// Choices offered for the Patch Editor size, in percent.
pub const EDITOR_SIZES: [u32; 5] = [100, 125, 150, 175, 200];
pub const DEFAULT_EDITOR_SIZE: u32 = 200;

impl Prefs {
    pub fn load() -> Self {
        // Still the original "EWItool" folder, so settings carry over
        // from before the EWITool2 rename.
        let file = directories::ProjectDirs::from("com.github", "ledhed2222", "EWItool")
            .map(|d| d.config_dir().join("prefs.txt"));
        let mut prefs = Self {
            file,
            ..Self::default()
        };
        let text = prefs
            .file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok());
        prefs.read(text.as_deref().unwrap_or(""));
        prefs
    }

    /// Apply the `key=value` lines of a prefs file.
    fn read(&mut self, text: &str) {
        let prefs = self;
        for line in text.lines() {
            match line.split_once('=') {
                Some(("library", v)) => prefs.library = Some(PathBuf::from(v)),
                Some(("midi_in", v)) => prefs.midi_in = Some(v.to_string()),
                Some(("midi_out", v)) => prefs.midi_out = Some(v.to_string()),
                Some(("theme", "dark")) => prefs.theme = ThemePreference::Dark,
                Some(("theme", "light")) => prefs.theme = ThemePreference::Light,
                Some(("editor_size", v)) => {
                    prefs.editor_size = v.parse().ok().filter(|n| EDITOR_SIZES.contains(n))
                }
                Some(("accent", v)) => prefs.look.accent = parse_hex_colour(v),
                Some(("corners", v)) => {
                    if let Ok(n) = v.parse::<u8>() {
                        prefs.look.corners = n.min(MAX_CORNERS);
                    }
                }
                Some(("text_size", v)) => {
                    if let Some(n) = v.parse().ok().filter(|n| TEXT_SIZES.contains(n)) {
                        prefs.look.text_scale = n as f32 / 100.0;
                    }
                }
                Some(("slider_fill", v)) => prefs.look.slider_fill = v != "0",
                _ => {}
            }
        }
    }

    /// Preferences that are never saved, starting with `library` and no
    /// MIDI ports.
    pub fn in_memory(library: Option<PathBuf>) -> Self {
        Self {
            library,
            ..Self::default()
        }
    }

    /// The patch set library folder, if one is chosen and still exists.
    pub fn library(&self) -> Option<&Path> {
        self.library.as_deref().filter(|p| p.is_dir())
    }

    pub fn set_library(&mut self, dir: PathBuf) -> io::Result<()> {
        self.library = Some(dir);
        self.save()
    }

    pub fn midi_in(&self) -> Option<&str> {
        self.midi_in.as_deref()
    }

    pub fn midi_out(&self) -> Option<&str> {
        self.midi_out.as_deref()
    }

    pub fn set_midi_ports(
        &mut self,
        midi_in: Option<String>,
        midi_out: Option<String>,
    ) -> io::Result<()> {
        self.midi_in = midi_in;
        self.midi_out = midi_out;
        self.save()
    }

    /// Dark, light, or follow the system (the default).
    pub fn theme(&self) -> ThemePreference {
        self.theme
    }

    pub fn set_theme(&mut self, theme: ThemePreference) -> io::Result<()> {
        self.theme = theme;
        self.save()
    }

    /// Patch Editor size in percent (default 200%).
    pub fn editor_size(&self) -> u32 {
        self.editor_size.unwrap_or(DEFAULT_EDITOR_SIZE)
    }

    pub fn set_editor_size(&mut self, percent: u32) -> io::Result<()> {
        self.editor_size = Some(percent);
        self.save()
    }

    /// Appearance settings (accent, corners, text size, slider fill).
    pub fn look(&self) -> Look {
        self.look
    }

    pub fn set_look(&mut self, look: Look) -> io::Result<()> {
        self.look = look;
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        let Some(file) = &self.file else {
            return Ok(());
        };
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut text = String::new();
        if let Some(lib) = &self.library {
            text.push_str(&format!("library={}\n", lib.display()));
        }
        if let Some(p) = &self.midi_in {
            text.push_str(&format!("midi_in={p}\n"));
        }
        if let Some(p) = &self.midi_out {
            text.push_str(&format!("midi_out={p}\n"));
        }
        match self.theme {
            ThemePreference::Dark => text.push_str("theme=dark\n"),
            ThemePreference::Light => text.push_str("theme=light\n"),
            ThemePreference::System => {}
        }
        if let Some(n) = self.editor_size {
            text.push_str(&format!("editor_size={n}\n"));
        }
        // Only settings changed from the defaults are written.
        let (look, default) = (self.look, Look::default());
        if let Some(c) = look.accent {
            text.push_str(&format!("accent={:02x}{:02x}{:02x}\n", c.r(), c.g(), c.b()));
        }
        if look.corners != default.corners {
            text.push_str(&format!("corners={}\n", look.corners));
        }
        if look.text_scale != default.text_scale {
            text.push_str(&format!(
                "text_size={}\n",
                (look.text_scale * 100.0).round()
            ));
        }
        if !look.slider_fill {
            text.push_str("slider_fill=0\n");
        }
        std::fs::write(file, text)
    }
}

/// Parse "rrggbb".
fn parse_hex_colour(s: &str) -> Option<Color32> {
    let n = u32::from_str_radix(s.trim(), 16)
        .ok()
        .filter(|_| s.trim().len() == 6)?;
    Some(Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn look_round_trips_through_the_prefs_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("prefs.txt");
        let look = Look {
            accent: Some(Color32::from_rgb(0x12, 0xA1, 0x8F)),
            corners: 3,
            text_scale: 1.15,
            slider_fill: false,
        };
        let mut prefs = Prefs {
            file: Some(file.clone()),
            ..Prefs::default()
        };
        prefs.set_look(look).unwrap();
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("accent=12a18f"));

        let mut loaded = Prefs::default();
        loaded.read(&std::fs::read_to_string(&file).unwrap());
        assert_eq!(loaded.look, look);
    }

    #[test]
    fn bad_colours_are_ignored() {
        assert_eq!(parse_hex_colour("zzzzzz"), None);
        assert_eq!(parse_hex_colour("fff"), None);
        assert_eq!(
            parse_hex_colour("ff8000"),
            Some(Color32::from_rgb(255, 128, 0))
        );
    }
}
