//! Colours, text sizes and the few building blocks every screen uses.

use eframe::egui::{self, Color32, FontId, RichText, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(255, 196, 0); // bumblebee yellow, for Hommel
pub const RECORDING: Color32 = Color32::from_rgb(235, 64, 64);
pub const WARNING: Color32 = Color32::from_rgb(240, 140, 50);
pub const WEAK: Color32 = Color32::from_rgb(150, 150, 160);
pub const TEXT: Color32 = Color32::from_rgb(232, 232, 236);
const BACKGROUND: Color32 = Color32::from_rgb(22, 22, 26);
const CARD: Color32 = Color32::from_rgb(36, 36, 42);
const CARD_SELECTED: Color32 = Color32::from_rgb(52, 48, 32);

/// PlayStation button symbols in their usual colours.
pub const CROSS: (&str, Color32) = ("✕", Color32::from_rgb(124, 178, 232));
pub const CIRCLE: (&str, Color32) = ("○", Color32::from_rgb(255, 102, 102));
pub const TRIANGLE: (&str, Color32) = ("△", Color32::from_rgb(64, 226, 160));
pub const SQUARE: (&str, Color32) = ("□", Color32::from_rgb(255, 105, 248));
pub const R1: (&str, Color32) = ("R1", TEXT);
pub const OPTIONS: (&str, Color32) = ("OPTIONS", TEXT);
pub const DPAD: (&str, Color32) = ("D-pad", TEXT);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(26.0)),
            (TextStyle::Body, FontId::proportional(18.0)),
            (TextStyle::Button, FontId::proportional(18.0)),
            (TextStyle::Small, FontId::proportional(14.0)),
            (TextStyle::Monospace, FontId::monospace(16.0)),
        ]
        .into();
        style.spacing.item_spacing = egui::vec2(10.0, 6.0);
        style.visuals.panel_fill = BACKGROUND;
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.selection.bg_fill = ACCENT;
    });
}

/// A rounded panel; the selected one gets a yellow edge.
pub fn card<R>(ui: &mut egui::Ui, selected: bool, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let (fill, stroke) = if selected {
        (CARD_SELECTED, Stroke::new(2.0, ACCENT))
    } else {
        (CARD, Stroke::NONE)
    };
    return egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(14, 5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            return add(ui);
        })
        .inner;
}

/// Small grey capital letters above a group of rows.
pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(title.to_uppercase()).small().strong().color(WEAK));
}

/// A rounded tag, e.g. the speaking style.
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::new()
        .stroke(Stroke::new(1.5, color))
        .corner_radius(14)
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| return ui.label(RichText::new(text).color(color)));
}

/// A row of "button: what it does" hints.
pub fn hints(ui: &mut egui::Ui, items: &[((&str, Color32), &str)]) {
    ui.horizontal_wrapped(|ui| {
        for ((symbol, color), meaning) in items {
            ui.label(RichText::new(*symbol).strong().color(*color));
            ui.label(RichText::new(*meaning).color(WEAK));
            ui.add_space(14.0);
        }
    });
}

pub fn meter(ui: &mut egui::Ui, level: f32, recording: bool) {
    let fill = if recording { RECORDING } else { WEAK };
    ui.add(
        egui::ProgressBar::new(level.clamp(0.0, 1.0))
            .desired_width(420.0)
            .desired_height(10.0)
            .fill(fill),
    );
}
