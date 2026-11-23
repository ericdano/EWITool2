//! The Patch Editor's parameter panels (port of the `Ui*Grid` classes).
//!
//! Every control edits the patch directly and records a [`LiveControl`]
//! naming the EWI parameter it changed (NRPN number + index), taken from the
//! Java `sendLiveControl(index, nrpn, value)` calls. The MIDI step will send
//! these; for now nothing does.

use std::ops::RangeInclusive;

use eframe::egui::{self, ComboBox, RichText, Slider, SliderClamping, Ui};
use ewitool_core::patch::{Filter, Osc};
use ewitool_core::Patch;

/// One parameter change to send to the EWI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LiveControl {
    pub nrpn: u8,
    pub index: u8,
    pub value: u8,
}

// NRPN numbers of each parameter section.
const NRPN_OSC1: u8 = 64;
const NRPN_OSC2: u8 = 65;
const NRPN_OSC_FILTER1: u8 = 72;
const NRPN_OSC_FILTER2: u8 = 73;
const NRPN_NOISE_FILTER1: u8 = 74;
const NRPN_NOISE_FILTER2: u8 = 75;
const NRPN_ANTI_ALIAS: u8 = 79;
const NRPN_NOISE: u8 = 80;
const NRPN_MISC: u8 = 81;
const NRPN_AMP: u8 = 88;
const NRPN_CHORUS: u8 = 112;
const NRPN_DELAY: u8 = 113;
const NRPN_REVERB: u8 = 114;

const FULL: RangeInclusive<u8> = 0..=127;

/// Draws controls and records what changed.
struct Ctl<'a> {
    out: &'a mut Vec<LiveControl>,
    /// Current zoom of the editor (see [`patch_editor`]).
    scale: f32,
}

impl Ctl<'_> {
    fn emit(&mut self, nrpn: u8, index: u8, value: u8) {
        self.out.push(LiveControl { nrpn, index, value });
    }

    /// Horizontal slider with "Label value" above it.
    fn slider(
        &mut self,
        ui: &mut Ui,
        label: &str,
        v: &mut u8,
        range: RangeInclusive<u8>,
        nrpn: u8,
        index: u8,
    ) {
        ui.vertical(|ui| {
            let name = ui.label(RichText::new(format!("{label} {v}")).small()).id;
            // A slider's handle is sized from the control height; keep
            // handles a little smaller than buttons for a lighter look.
            ui.spacing_mut().interact_size.y *= SLIDER_HANDLE;
            // Only clamp values the user moves: patches from the EWI can hold
            // values outside the editor's ranges and must not change just
            // by being displayed.
            let r = ui
                .add(
                    Slider::new(v, range)
                        .show_value(false)
                        .clamping(SliderClamping::Edits),
                )
                .labelled_by(name);
            if r.changed() {
                self.emit(nrpn, index, *v);
            }
        });
    }

    /// Vertical 0-127 slider with the label and value above it.
    fn vslider(&mut self, ui: &mut Ui, label: &str, v: &mut u8, nrpn: u8, index: u8) {
        // A fixed-width column; a centred layout would otherwise take the
        // full available width.
        ui.vertical(|ui| {
            ui.set_width(36.0 * self.scale);
            ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                let name = ui.label(RichText::new(format!("{label}\n{v}")).small()).id;
                ui.spacing_mut().interact_size.y *= SLIDER_HANDLE;
                let r = ui
                    .add(
                        Slider::new(v, FULL)
                            .vertical()
                            .show_value(false)
                            .clamping(SliderClamping::Edits),
                    )
                    .labelled_by(name);
                if r.changed() {
                    self.emit(nrpn, index, *v);
                }
            });
        });
    }

    /// Drop-down where option `i` stores the value `offset + i`.
    #[allow(clippy::too_many_arguments)]
    fn choice(
        &mut self,
        ui: &mut Ui,
        label: &str,
        v: &mut u8,
        options: &[String],
        offset: u8,
        nrpn: u8,
        index: u8,
    ) {
        let current = v
            .checked_sub(offset)
            .map(usize::from)
            .filter(|&i| i < options.len());
        let text = current.map_or_else(|| format!("? ({v})"), |i| options[i].clone());
        let mut picked = current;
        ui.vertical(|ui| {
            let name = ui.label(RichText::new(label).small()).id;
            ComboBox::from_id_salt(label)
                .selected_text(text)
                .width(80.0 * self.scale)
                .show_ui(ui, |ui| {
                    for (i, o) in options.iter().enumerate() {
                        ui.selectable_value(&mut picked, Some(i), o);
                    }
                })
                .response
                .labelled_by(name);
        });
        if let Some(i) = picked.filter(|_| picked != current) {
            *v = offset + i as u8;
            self.emit(nrpn, index, *v);
        }
    }

    /// On/off switch stored as 1/0.
    fn check(&mut self, ui: &mut Ui, label: &str, v: &mut u8, nrpn: u8, index: u8) {
        let mut on = *v == 1;
        if ui.checkbox(&mut on, label).changed() {
            *v = u8::from(on);
            self.emit(nrpn, index, *v);
        }
    }
}

fn opts(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// Signed labels, e.g. -2..=2 -> ["-2", "-1", "0", "+1", "+2"].
fn signed(range: RangeInclusive<i32>) -> Vec<String> {
    range
        .map(|n| {
            if n > 0 {
                format!("+{n}")
            } else {
                n.to_string()
            }
        })
        .collect()
}

/// A parameter group: a card with a title.
fn panel(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    // The editor's current zoom, recovered from the slider length it set.
    let scale = ui.spacing().slider_width / SLIDER_WIDTH;
    let title_size = ui.style().text_styles[&egui::TextStyle::Body].size * 1.15;
    ui.push_id(title, |ui| {
        crate::theme::card(ui, scale).show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(crate::theme::title(title, title_size));
                add(ui);
            });
        });
    });
}

/// A column of controls with an optional small section heading.
fn column(ui: &mut Ui, heading: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        ui.label(crate::theme::section(heading));
        add(ui);
    });
}

fn osc_panel(ctl: &mut Ctl, ui: &mut Ui, title: &str, o: &mut Osc, n: u8, xfade: Option<&mut u8>) {
    panel(ui, title, |ui| {
        if let Some(x) = xfade {
            ctl.check(ui, "Cross-fade", x, NRPN_MISC, 6);
        }
        ui.horizontal_top(|ui| {
            column(ui, "Pitch", |ui| {
                ctl.choice(ui, "Octave", &mut o.octave, &signed(-2..=2), 62, n, 0);
                ctl.slider(ui, "Fine", &mut o.fine, 14..=114, n, 2);
                ctl.choice(ui, "Semitone", &mut o.semitone, &signed(-12..=12), 52, n, 1);
                ctl.slider(ui, "Beat", &mut o.beat, FULL, n, 3);
            });
            ctl.vslider(ui, "Saw", &mut o.sawtooth, n, 5);
            ctl.vslider(ui, "Tri", &mut o.triangle, n, 6);
            ctl.vslider(ui, "Sqr", &mut o.square, n, 7);
            column(ui, "PWM", |ui| {
                ctl.slider(ui, "Width", &mut o.pulse_width, FULL, n, 8);
                ctl.slider(ui, "Freq", &mut o.pwm_freq, FULL, n, 9);
                ctl.slider(ui, "Depth", &mut o.pwm_depth, FULL, n, 10);
            });
            column(ui, "Sweep", |ui| {
                ctl.slider(ui, "Time", &mut o.sweep_time, FULL, n, 12);
                ctl.slider(ui, "Depth", &mut o.sweep_depth, FULL, n, 11);
            });
            column(ui, "Breath", |ui| {
                ctl.slider(ui, "Attain", &mut o.breath_attain, FULL, n, 14);
                ctl.slider(ui, "Thresh", &mut o.breath_threshold, FULL, n, 16);
                ctl.slider(ui, "Curve", &mut o.breath_curve, FULL, n, 15);
                ctl.slider(ui, "Depth", &mut o.breath_depth, FULL, n, 13);
            });
            ctl.vslider(ui, "Vol", &mut o.level, n, 17);
        });
    });
}

/// `link` is the routing setting (and its misc-section index) shown only on
/// primary filters.
fn filter_panel(
    ctl: &mut Ctl,
    ui: &mut Ui,
    title: &str,
    f: &mut Filter,
    n: u8,
    link: Option<(&mut u8, u8)>,
) {
    panel(ui, title, |ui| {
        ui.horizontal_top(|ui| {
            column(ui, "", |ui| {
                if let Some((v, index)) = link {
                    let routes = opts(&["1-only", "Linked", "Both"]);
                    ctl.choice(ui, "Routing", v, &routes, 0, NRPN_MISC, index);
                }
                let types = opts(&["Low Pass", "High Pass", "Band Pass", "Notch", "Off"]);
                ctl.choice(ui, "Type", &mut f.mode, &types, 0, n, 0);
                ctl.slider(ui, "Key Follow", &mut f.key_follow, 52..=88, n, 3);
            });
            column(ui, "", |ui| {
                ctl.slider(ui, "Q", &mut f.q, 5..=127, n, 2);
                ctl.slider(ui, "Cutoff Freq", &mut f.freq, 36..=124, n, 1);
            });
            column(ui, "Breath", |ui| {
                ctl.slider(ui, "Mod", &mut f.breath_mod, FULL, n, 4);
                ctl.slider(ui, "Curve", &mut f.breath_curve, FULL, n, 11);
            });
            column(ui, "LFO", |ui| {
                ctl.slider(ui, "Freq", &mut f.lfo_freq, FULL, n, 5);
                ctl.slider(ui, "Depth", &mut f.lfo_depth, FULL, n, 6);
            });
            column(ui, "LFO", |ui| {
                ctl.slider(ui, "Breath", &mut f.lfo_breath, FULL, n, 7);
                ctl.slider(ui, "Threshold", &mut f.lfo_threshold, FULL, n, 8);
            });
            column(ui, "Sweep", |ui| {
                ctl.slider(ui, "Time", &mut f.sweep_time, FULL, n, 10);
                ctl.slider(ui, "Depth", &mut f.sweep_depth, FULL, n, 9);
            });
        });
    });
}

/// Slider height relative to other controls (sets the handle size).
const SLIDER_HANDLE: f32 = 0.75;
/// Slider length at 100% size.
const SLIDER_WIDTH: f32 = 76.0;
/// Never shrink below this, even in a very narrow window (it scrolls
/// sideways instead).
const MIN_SCALE: f32 = 0.6;
/// Space between panels at 100% size.
const GAP: f32 = 8.0;

/// Scale every size in `ui`'s style (text, spacing, widget sizes).
fn scale_style(ui: &mut Ui, scale: f32) {
    let style = ui.style_mut();
    for font in style.text_styles.values_mut() {
        font.size *= scale;
    }
    let s = &mut style.spacing;
    s.item_spacing *= scale;
    s.button_padding *= scale;
    s.interact_size *= scale;
    s.slider_width = SLIDER_WIDTH * scale;
    s.slider_rail_height *= scale;
    s.icon_width *= scale;
    s.icon_width_inner *= scale;
    s.icon_spacing *= scale;
}

/// Flow layout for the editor's panels: left to right, starting a new row
/// when the next panel doesn't fit.
///
/// egui only learns a panel's size by drawing it, so each panel's size at
/// 100% is remembered from the previous frame and used to decide where it
/// goes. When a size changes (first frame, resize, size setting) the editor
/// redraws once more so everything lands in the right place.
struct Flow {
    origin: egui::Pos2,
    width: f32,
    scale: f32,
    x: f32,
    y: f32,
    row_height: f32,
    /// Widest panel at 100%, to choose the scale next frame.
    widest: f32,
    changed: bool,
}

impl Flow {
    fn new(ui: &Ui, scale: f32) -> Self {
        Self {
            origin: ui.cursor().min,
            width: ui.available_width(),
            scale,
            x: 0.0,
            y: 0.0,
            row_height: 0.0,
            widest: 0.0,
            changed: false,
        }
    }

    fn place(&mut self, ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
        let size_id = egui::Id::unique(("editor_panel_size", id));
        let known: Option<egui::Vec2> = ui.data(|d| d.get_temp(size_id));
        let expected = known.unwrap_or(egui::vec2(200.0, 100.0)) * self.scale;
        let gap = GAP * self.scale;
        if self.x > 0.0 && self.x + expected.x > self.width {
            self.x = 0.0;
            self.y += self.row_height + gap;
            self.row_height = 0.0;
        }
        let min = self.origin + egui::vec2(self.x, self.y);
        let max_width = (self.width - self.x).max(expected.x);
        let rect = egui::Rect::from_min_size(min, egui::vec2(max_width, f32::INFINITY));
        let builder = egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min));
        let size = ui.scope_builder(builder, add).response.rect.size();

        let natural = size / self.scale;
        if known.is_none_or(|k| (k - natural).length() > 1.0) {
            ui.data_mut(|d| d.insert_temp(size_id, natural));
            self.changed = true;
        }
        self.x += size.x + gap;
        self.row_height = self.row_height.max(size.y);
        self.widest = self.widest.max(natural.x);
    }
}

/// Draw the whole editor for `p`, appending changed parameters to `out`.
///
/// `size` is the preferred size (1.0 = 100%). Panels flow into as many
/// columns as fit the window's width, and the editor scrolls vertically.
/// The size is only reduced when the window is too narrow for the widest
/// panel.
pub fn patch_editor(ui: &mut Ui, p: &mut Patch, out: &mut Vec<LiveControl>, size: f32) {
    let widest_id = ui.scope_id().with("editor_widest_panel");
    let widest: Option<f32> = ui.data(|d| d.get_temp(widest_id));
    // Leave room for the vertical scroll bar.
    let width = ui.available_width() - 20.0;
    let scale = widest.map_or(size, |w| size.min(width / w)).max(MIN_SCALE);
    scale_style(ui, scale);

    let mut ctl = Ctl { out, scale };
    let ctl = &mut ctl;

    let mut flow = None;
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        let f = flow.insert(Flow::new(ui, scale));
        f.width = f.width.min(width);

        f.place(ui, "osc1", |ui| {
            osc_panel(ctl, ui, "Oscillator 1", &mut p.osc1, NRPN_OSC1, None);
        });
        f.place(ui, "osc2", |ui| {
            let xfade = Some(&mut p.misc.osc2_xfade);
            osc_panel(ctl, ui, "Oscillator 2", &mut p.osc2, NRPN_OSC2, xfade);
        });
        f.place(ui, "formant_key", |ui| {
            panel(ui, "Formant Filter", |ui| {
                let o = opts(&["Off", "Woodwind", "Strings"]);
                ctl.choice(ui, "Type", &mut p.misc.formant_filter, &o, 0, NRPN_MISC, 5);
            });
            panel(ui, "Key Trigger", |ui| {
                let o = opts(&["Single", "Multi"]);
                ctl.choice(ui, "Mode", &mut p.misc.key_trigger, &o, 0, NRPN_MISC, 7);
            });
        });
        f.place(ui, "osc_filter1", |ui| {
            let link = Some((&mut p.misc.osc_filter_link, 3));
            let f1 = &mut p.osc_filter1;
            filter_panel(ctl, ui, "Primary Osc Filter", f1, NRPN_OSC_FILTER1, link);
        });
        f.place(ui, "osc_filter2", |ui| {
            let f2 = &mut p.osc_filter2;
            filter_panel(ctl, ui, "Secondary Osc Filter", f2, NRPN_OSC_FILTER2, None);
        });
        f.place(ui, "noise", |ui| {
            panel(ui, "Noise", |ui| {
                ui.horizontal_top(|ui| {
                    column(ui, "", |ui| {
                        ctl.slider(ui, "Time", &mut p.noise.time, FULL, NRPN_NOISE, 0);
                        ctl.slider(ui, "Breath", &mut p.noise.breath, FULL, NRPN_NOISE, 1);
                    });
                    ctl.vslider(ui, "Vol", &mut p.noise.level, NRPN_NOISE, 2);
                });
            });
        });
        f.place(ui, "noise_filter1", |ui| {
            let link = Some((&mut p.misc.noise_filter_link, 4));
            let f1 = &mut p.noise_filter1;
            filter_panel(
                ctl,
                ui,
                "Primary Noise Filter",
                f1,
                NRPN_NOISE_FILTER1,
                link,
            );
        });
        f.place(ui, "noise_filter2", |ui| {
            let f2 = &mut p.noise_filter2;
            filter_panel(
                ctl,
                ui,
                "Secondary Noise Filter",
                f2,
                NRPN_NOISE_FILTER2,
                None,
            );
        });
        f.place(ui, "chorus", |ui| {
            panel(ui, "Chorus", |ui| {
                let n = NRPN_CHORUS;
                ctl.check(ui, "Enable", &mut p.misc.chorus_switch, NRPN_MISC, 9);
                let c = &mut p.chorus;
                ui.horizontal_top(|ui| {
                    column(ui, "", |ui| {
                        ctl.slider(ui, "Delay 1", &mut c.delay1, FULL, n, 0);
                        ctl.slider(ui, "Delay 2", &mut c.delay2, FULL, n, 3);
                    });
                    column(ui, "", |ui| {
                        ctl.slider(ui, "Vib 1", &mut c.mod_lev1, FULL, n, 1);
                        ctl.slider(ui, "Vib 2", &mut c.mod_lev2, FULL, n, 4);
                    });
                    column(ui, "", |ui| {
                        ctl.slider(ui, "Wet 1", &mut c.wet_lev1, FULL, n, 2);
                        ctl.slider(ui, "Wet 2", &mut c.wet_lev2, FULL, n, 5);
                    });
                    ctl.vslider(ui, "Dry", &mut c.dry_level, n, 8);
                    column(ui, "", |ui| {
                        // Java sent index 62 here, almost certainly a typo for
                        // 6 (feedback's position in the section).
                        ctl.slider(ui, "Feedback", &mut c.feedback, FULL, n, 6);
                        ctl.slider(ui, "LFO Freq", &mut c.lfo_freq, FULL, n, 7);
                    });
                });
            });
        });
        f.place(ui, "delay", |ui| {
            panel(ui, "Delay", |ui| {
                let (d, n) = (&mut p.delay, NRPN_DELAY);
                ui.horizontal_top(|ui| {
                    column(ui, "", |ui| {
                        ctl.slider(ui, "Time", &mut d.time, FULL, n, 0);
                        ctl.slider(ui, "Damping", &mut d.damp, FULL, n, 2);
                    });
                    column(ui, "", |ui| {
                        ctl.slider(ui, "Feedback", &mut d.feedback, FULL, n, 1);
                        ctl.slider(ui, "Dry", &mut d.dry, FULL, n, 4);
                    });
                    ctl.vslider(ui, "Vol", &mut d.level, n, 3);
                });
            });
        });
        f.place(ui, "reverb", |ui| {
            panel(ui, "Reverb", |ui| {
                let (r, n) = (&mut p.reverb, NRPN_REVERB);
                ui.horizontal_top(|ui| {
                    column(ui, "", |ui| {
                        ctl.slider(ui, "Time", &mut r.time, 10..=50, n, 3);
                        ctl.slider(ui, "Damping", &mut r.damp, 54..=74, n, 4);
                    });
                    column(ui, "", |ui| {
                        ctl.slider(ui, "Density", &mut r.density, FULL, n, 2);
                        ctl.slider(ui, "Dry", &mut r.dry, FULL, n, 0);
                    });
                    ctl.vslider(ui, "Vol", &mut r.level, n, 1);
                });
            });
        });
        f.place(ui, "bite", |ui| {
            panel(ui, "Bite", |ui| {
                ctl.slider(ui, "Vibrato", &mut p.misc.bite_vibrato, FULL, NRPN_MISC, 2);
                ctl.slider(ui, "Tremolo", &mut p.amp.bite_tremolo, FULL, NRPN_AMP, 0);
            });
        });
        f.place(ui, "pitch_bend", |ui| {
            panel(ui, "Pitch Bend", |ui| {
                ctl.check(ui, "Step Mode", &mut p.misc.bend_step_mode, NRPN_MISC, 1);
                let ranges: Vec<String> = (0..=12).map(|n| n.to_string()).collect();
                ctl.choice(
                    ui,
                    "Range",
                    &mut p.misc.bend_range,
                    &ranges,
                    0,
                    NRPN_MISC,
                    0,
                );
            });
        });
        f.place(ui, "anti_alias", |ui| {
            panel(ui, "Anti-Alias", |ui| {
                let (a, n) = (&mut p.anti_alias, NRPN_ANTI_ALIAS);
                ctl.check(ui, "Enable", &mut a.switch, n, 0);
                ctl.slider(ui, "Cutoff Freq", &mut a.cutoff, FULL, n, 1);
                ctl.slider(ui, "Key Follow", &mut a.key_follow, FULL, n, 2);
            });
        });
        f.place(ui, "levels", |ui| {
            panel(ui, "Levels", |ui| {
                ctl.slider(ui, "Master", &mut p.amp.level, FULL, NRPN_AMP, 1);
                ctl.slider(ui, "Octave", &mut p.amp.octave_level, FULL, NRPN_AMP, 2);
            });
        });
    });

    let flow = flow.expect("flow was laid out");
    if widest.is_none_or(|w| (w - flow.widest).abs() > 1.0) {
        ui.data_mut(|d| d.insert_temp(widest_id, flow.widest));
        ui.ctx().request_repaint();
    }
    if flow.changed {
        ui.ctx().request_repaint();
    }
}
