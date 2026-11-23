//! EWITool2's look: the Inter font, card-style panels, one accent colour and
//! matching light and dark palettes.
//!
//! Everything visual that isn't layout lives here, so the look can be
//! changed in one place. Call [`install`] once with the app's context.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Margin,
    RichText, Shadow, Stroke, TextStyle, Theme, Ui, Visuals,
};

/// The colours for one theme.
#[derive(Clone, Copy)]
pub struct Palette {
    /// Window background.
    pub bg: Color32,
    /// Cards: panels, lists, dialogs.
    pub surface: Color32,
    /// Controls at rest (buttons, slider rails, drop-downs).
    pub control: Color32,
    /// Controls under the mouse.
    pub hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub accent: Color32,
    /// Text on top of the accent colour.
    pub on_accent: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub shadow: Color32,
}

pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x12, 0x14, 0x18),
    surface: Color32::from_rgb(0x1B, 0x1E, 0x24),
    control: Color32::from_rgb(0x29, 0x2D, 0x35),
    hover: Color32::from_rgb(0x33, 0x38, 0x42),
    border: Color32::from_rgb(0x2C, 0x31, 0x3A),
    text: Color32::from_rgb(0xE6, 0xE8, 0xEC),
    text_weak: Color32::from_rgb(0x95, 0x9C, 0xA8),
    accent: Color32::from_rgb(0x4C, 0x8D, 0xF6),
    on_accent: Color32::WHITE,
    good: Color32::from_rgb(0x34, 0xC7, 0x6F),
    warn: Color32::from_rgb(0xF5, 0xA5, 0x24),
    shadow: Color32::from_black_alpha(90),
};

pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xF2, 0xF3, 0xF6),
    surface: Color32::WHITE,
    control: Color32::from_rgb(0xEC, 0xEE, 0xF2),
    hover: Color32::from_rgb(0xE0, 0xE4, 0xEA),
    border: Color32::from_rgb(0xDD, 0xE1, 0xE7),
    text: Color32::from_rgb(0x1C, 0x20, 0x26),
    text_weak: Color32::from_rgb(0x67, 0x6F, 0x7C),
    accent: Color32::from_rgb(0x25, 0x63, 0xEB),
    on_accent: Color32::WHITE,
    good: Color32::from_rgb(0x16, 0xA3, 0x4A),
    warn: Color32::from_rgb(0xB4, 0x53, 0x09),
    shadow: Color32::from_black_alpha(28),
};

/// The user's appearance choices (File ▸ Preferences).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// `None` uses the default blue (tuned separately for light and dark).
    pub accent: Option<Color32>,
    /// Corner radius of controls, in points; cards and windows are rounder.
    pub corners: u8,
    /// Text size, 1.0 = 100%.
    pub text_scale: f32,
    /// Fill slider tracks up to the handle.
    pub slider_fill: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            accent: None,
            corners: DEFAULT_CORNERS,
            text_scale: 1.0,
            slider_fill: true,
        }
    }
}

pub const DEFAULT_CORNERS: u8 = 8;
pub const MAX_CORNERS: u8 = 16;

/// Accent colour choices offered in Preferences (besides the default blue).
pub const ACCENTS: [(&str, Color32); 6] = [
    ("Teal", Color32::from_rgb(0x12, 0xA1, 0x8F)),
    ("Purple", Color32::from_rgb(0x7C, 0x4D, 0xFF)),
    ("Pink", Color32::from_rgb(0xD6, 0x33, 0x6C)),
    ("Orange", Color32::from_rgb(0xE8, 0x63, 0x1A)),
    ("Green", Color32::from_rgb(0x2F, 0x9E, 0x44)),
    ("Graphite", Color32::from_rgb(0x5C, 0x63, 0x70)),
];

/// The default accent for a theme.
pub fn default_accent(dark: bool) -> Color32 {
    if dark {
        DARK.accent
    } else {
        LIGHT.accent
    }
}

/// Black or white, whichever reads better on `bg`.
pub fn text_on(bg: Color32) -> Color32 {
    let lum = 0.2126 * f32::from(bg.r()) + 0.7152 * f32::from(bg.g()) + 0.0722 * f32::from(bg.b());
    if lum > 160.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// The palette for whichever theme `ui` is showing, with the user's accent.
pub fn palette(ui: &Ui) -> Palette {
    let mut p = if ui.visuals().dark_mode { DARK } else { LIGHT };
    p.accent = ui.visuals().selection.bg_fill;
    p.on_accent = text_on(p.accent);
    p
}

/// Font family for headings (Inter SemiBold).
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Set up the fonts (once) and both themes' styles.
pub fn install(ctx: &egui::Context, look: Look) {
    install_fonts(ctx);
    apply(ctx, look);
}

/// Restyle both themes for `look` (cheap; call whenever it changes).
pub fn apply(ctx: &egui::Context, look: Look) {
    let t = look.text_scale;
    for (theme, p) in [(Theme::Dark, DARK), (Theme::Light, LIGHT)] {
        ctx.style_mut_of(theme, |style| {
            style.visuals = visuals(theme, p, look);
            style.text_styles = [
                (TextStyle::Small, FontId::proportional(11.5 * t)),
                (TextStyle::Body, FontId::proportional(14.0 * t)),
                (TextStyle::Button, FontId::proportional(14.0 * t)),
                (TextStyle::Heading, FontId::new(19.0 * t, semibold())),
                (TextStyle::Monospace, FontId::monospace(13.0 * t)),
            ]
            .into();
            let s = &mut style.spacing;
            s.item_spacing = egui::vec2(8.0, 7.0);
            s.button_padding = egui::vec2(11.0, 5.0);
            s.interact_size = egui::vec2(40.0, 26.0);
            s.menu_margin = Margin::same(8);
            s.window_margin = Margin::same(18);
            s.slider_rail_height = 6.0;
        });
    }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.ttf"
        ))),
    );
    // Inter first; egui's own fonts stay behind it for symbols Inter lacks.
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".into());
    let mut bold = vec!["Inter-SemiBold".to_string()];
    bold.extend(fallbacks);
    fonts.families.insert(semibold(), bold);
    ctx.set_fonts(fonts);
}

fn visuals(theme: Theme, mut p: Palette, look: Look) -> Visuals {
    let mut v = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    let r = |n: u8| CornerRadius::same(n);
    let c = look.corners;
    if let Some(accent) = look.accent {
        p.accent = accent;
        p.on_accent = text_on(accent);
    }

    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = r(c + 6);
    v.menu_corner_radius = r(c + 2);
    v.window_shadow = Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: p.shadow,
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 16,
        spread: 0,
        color: p.shadow,
    };
    v.extreme_bg_color = match theme {
        Theme::Dark => Color32::from_rgb(0x0E, 0x10, 0x13),
        Theme::Light => Color32::WHITE,
    };
    v.faint_bg_color = p.control;
    v.warn_fg_color = p.warn;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.on_accent);
    v.slider_trailing_fill = look.slider_fill;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.striped = false;

    let w = &mut v.widgets;
    // Labels and frames.
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    // Cards read their rounding from here (see `card`).
    w.noninteractive.corner_radius = r(c + 4);
    // Controls at rest, under the mouse, pressed, and open (drop-downs).
    for (state, fill, stroke) in [
        (&mut w.inactive, p.control, Stroke::NONE),
        (
            &mut w.hovered,
            p.hover,
            Stroke::new(1.0, p.accent.gamma_multiply(0.6)),
        ),
        (&mut w.active, p.hover, Stroke::new(1.0, p.accent)),
        (&mut w.open, p.hover, Stroke::new(1.0, p.border)),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = stroke;
        state.fg_stroke = Stroke::new(1.5, p.text);
        state.corner_radius = r(c);
        state.expansion = 0.0;
    }
    v
}

/// A card: the rounded, slightly raised background for panels and lists.
/// `scale` grows the padding with the Patch Editor's size.
pub fn card(ui: &Ui, scale: f32) -> Frame {
    let p = palette(ui);
    let radius = f32::from(ui.visuals().widgets.noninteractive.corner_radius.nw) * scale;
    Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(CornerRadius::same(radius.min(255.0) as u8))
        .inner_margin(Margin::same((12.0 * scale) as i8))
        .shadow(Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: p.shadow.gamma_multiply(0.5),
        })
}

/// A heading in the semibold font.
pub fn title(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).family(semibold()).size(size)
}

/// A small, muted, upper-case section label (e.g. "PWM", "LFO").
pub fn section(text: &str) -> RichText {
    RichText::new(text.to_uppercase()).small().weak()
}

/// The main button of a dialog, filled with the accent colour.
pub fn primary_button(ui: &Ui, text: &str) -> egui::Button<'static> {
    let p = palette(ui);
    egui::Button::new(RichText::new(text.to_string()).color(p.on_accent)).fill(p.accent)
}

/// A small filled circle, e.g. for connection status.
pub fn dot(ui: &mut Ui, color: Color32) {
    let size = ui.spacing().interact_size.y * 0.35;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
}
