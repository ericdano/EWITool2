//! The native macOS menu bar (via muda), replacing the in-window menus.
//!
//! Menu clicks arrive on AppKit's side; they are forwarded through a
//! channel as [`Command`]s and the window is woken to handle them. Each
//! frame the app passes its [`MenuState`] so checkmarks, enabled items and
//! the Store label stay current.
//!
//! Only ⌘, (Preferences), ⌘S (Store) and the standard Hide/Quit/Minimize
//! keys are bound here. ⌘Z and ⌘C stay with the window, because a native
//! menu item bound to them would take those keys from text boxes too.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver};

use eframe::egui;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::app::{Command, EditOp, MenuState, APP_NAME, GENERATE, PROCESS, THEMES};
use crate::prefs::EDITOR_SIZES;

pub struct NativeMenu {
    /// Kept alive for as long as the menu bar shows it.
    _menu: Menu,
    commands: HashMap<MenuId, Command>,
    received: Receiver<MenuId>,
    check_ewi: MenuItem,
    fetch_all: MenuItem,
    store: MenuItem,
    /// Items enabled only while editing a patch.
    patch_items: Vec<MenuItem>,
    patch_submenus: Vec<Submenu>,
    themes: Vec<(egui::ThemePreference, CheckMenuItem)>,
    sizes: Vec<(u32, CheckMenuItem)>,
    shown: Option<MenuState>,
}

fn cmd_key(code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(Modifiers::META, code))
}

impl NativeMenu {
    /// Build the menu bar and install it. Must run on the main thread
    /// after the app has started (e.g. in eframe's creation callback).
    pub fn install(ctx: &egui::Context) -> Self {
        let mut commands = HashMap::new();
        let mut item = |text: &str, cmd: Command, key: Option<Accelerator>| {
            let i = MenuItem::new(text, true, key);
            commands.insert(i.id().clone(), cmd);
            i
        };

        // App menu (titled with the app's name by macOS).
        let about = item(&format!("About {APP_NAME}"), Command::About, None);
        let prefs = item("Preferences…", Command::Preferences, cmd_key(Code::Comma));
        let app_menu = Submenu::new(APP_NAME, true);

        let ports = item("Ports…", Command::Ports, None);
        let check_ewi = item("Check for EWI", Command::CheckEwi, None);
        let fetch_all = item("Fetch All Patches", Command::FetchAll, None);

        let store = item(
            "Store to EWI",
            Command::Edit(EditOp::Store),
            cmd_key(Code::KeyS),
        );
        let revert = item("Revert Edits", Command::Edit(EditOp::Revert), None);
        let copy = item(
            "Copy to Patch Scratchpad",
            Command::Edit(EditOp::CopyToScratchpad),
            None,
        );
        let generate = Submenu::new("Generate", true);
        let process = Submenu::new("Process", true);
        let mut patch_items = vec![store.clone(), revert.clone(), copy.clone()];
        for (submenu, list) in [(&generate, &GENERATE[..]), (&process, &PROCESS[..])] {
            for &(label, op) in list {
                let i = item(label, Command::Edit(op), None);
                let _ = submenu.append(&i);
                patch_items.push(i);
            }
        }

        let help = item("Online Help", Command::OnlineHelp, None);

        let mut themes = Vec::new();
        let mut sizes = Vec::new();
        let theme_menu = Submenu::new("Theme", true);
        for (t, label) in THEMES {
            let c = CheckMenuItem::new(label, true, false, None);
            commands.insert(c.id().clone(), Command::Theme(t));
            let _ = theme_menu.append(&c);
            themes.push((t, c));
        }
        let size_menu = Submenu::new("Patch Editor Size", true);
        for n in EDITOR_SIZES {
            let c = CheckMenuItem::new(format!("{n}%"), true, false, None);
            commands.insert(c.id().clone(), Command::EditorSize(n));
            let _ = size_menu.append(&c);
            sizes.push((n, c));
        }

        let sep = PredefinedMenuItem::separator;
        let _ = app_menu.append_items(&[
            &about,
            &sep(),
            &prefs,
            &sep(),
            &PredefinedMenuItem::services(None),
            &sep(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &sep(),
            &PredefinedMenuItem::quit(None),
        ]);
        // Preferences and Quit live in the app menu on macOS.
        let file_menu = Submenu::new("File", true);
        let _ = file_menu.append(&PredefinedMenuItem::close_window(None));
        let midi_menu = Submenu::new("MIDI", true);
        let _ = midi_menu.append_items(&[&ports, &check_ewi]);
        let ewi_menu = Submenu::new("EWI", true);
        let _ = ewi_menu.append(&fetch_all);
        let patch_menu = Submenu::new("Patch", true);
        let _ = patch_menu.append_items(&[&store, &revert, &copy, &sep(), &generate, &process]);
        let view_menu = Submenu::new("View", true);
        let _ = view_menu.append_items(&[&theme_menu, &size_menu]);
        let window_menu = Submenu::new("Window", true);
        let _ = window_menu.append_items(&[
            &PredefinedMenuItem::minimize(None),
            &PredefinedMenuItem::maximize(None),
            &PredefinedMenuItem::fullscreen(None),
            &sep(),
            &PredefinedMenuItem::bring_all_to_front(None),
        ]);
        let help_menu = Submenu::new("Help", true);
        let _ = help_menu.append(&help);

        let menu = Menu::new();
        let _ = menu.append_items(&[
            &app_menu,
            &file_menu,
            &midi_menu,
            &ewi_menu,
            &patch_menu,
            &view_menu,
            &window_menu,
            &help_menu,
        ]);
        menu.init_for_nsapp();
        window_menu.set_as_windows_menu_for_nsapp();
        help_menu.set_as_help_menu_for_nsapp();

        let (tx, received) = mpsc::channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            let _ = tx.send(event.id);
            ctx.request_repaint();
        }));

        Self {
            _menu: menu,
            commands,
            received,
            check_ewi,
            fetch_all,
            store,
            patch_items,
            patch_submenus: vec![generate, process],
            themes,
            sizes,
            shown: None,
        }
    }

    /// Menu clicks since the last call.
    pub fn take_commands(&mut self) -> Vec<Command> {
        let cmds: Vec<Command> = self
            .received
            .try_iter()
            .filter_map(|id| self.commands.get(&id).copied())
            .collect();
        if !cmds.is_empty() {
            // Clicking a check item flips it; put the checkmarks right.
            self.shown = None;
        }
        cmds
    }

    /// Bring enabled items, checkmarks and labels up to date.
    pub fn sync(&mut self, state: &MenuState) {
        if self.shown.as_ref() == Some(state) {
            return;
        }
        self.check_ewi.set_enabled(state.check_ewi);
        self.fetch_all.set_enabled(state.fetch_all);
        for item in &self.patch_items {
            item.set_enabled(state.patch);
        }
        for submenu in &self.patch_submenus {
            submenu.set_enabled(state.patch);
        }
        self.store.set_text(state.store_label);
        for (t, item) in &self.themes {
            item.set_checked(*t == state.theme);
        }
        for (n, item) in &self.sizes {
            item.set_checked(*n == state.editor_size);
        }
        self.shown = Some(state.clone());
    }
}
