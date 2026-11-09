//! The conversation: bubbles, rendered answers and what sits under them.

use egui::{Align, CornerRadius, Layout, Stroke};

use super::native::{Actions, Msg, Source, Who};
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

pub(super) fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, act: &mut Actions) {
    let running = app.ai.running();
    let has_doc = app.active_ids().is_some();
    egui::ScrollArea::vertical().id_salt("pdf-ai-conversation").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if app.ai.messages.is_empty() {
            welcome(ui, t, !running && has_doc, act);
            return;
        }
        ui.add_space(5.0);
        let last = app.ai.messages.len().saturating_sub(1);
        for (i, m) in app.ai.messages.iter_mut().enumerate() {
            match m.who {
                Who::User => user_bubble(ui, t, m),
                Who::Assistant => assistant(ui, t, m, i, i == last, running, act),
                Who::Error => error_bubble(ui, t, m, i == last && !running, act),
            }
            ui.add_space(12.0);
        }
    });
}

fn welcome(ui: &mut egui::Ui, t: &Tokens, enabled: bool, act: &mut Actions) {
    ui.add_space((ui.available_height() * 0.08).clamp(10.0, 60.0));
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(54.0, 54.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, CornerRadius::same(18), t.accent_soft);
        icons::paint(ui, rect, "sparkles", 26.0, t.accent_text);
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tl!("What would you like to know?")).font(theme::semibold(17.0)).color(t.text));
        ui.add_space(2.0);
        ui.label(egui::RichText::new(tl!("Ask a question or choose a suggestion.")).font(theme::regular(12.5)).color(t.text_muted));
    });
    ui.add_space(16.0);
    if let Some(request) = super::composer::suggestions(ui, t, enabled) {
        act.send = Some(request);
    }
}

fn user_bubble(ui: &mut egui::Ui, t: &Tokens, m: &Msg) {
    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
        ui.vertical(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(egui::RichText::new(tl!("You")).font(theme::medium(11.5)).color(t.text_faint));
                if let Some(tag) = m.ask.as_ref().and_then(|(_, s)| source_tag(s)) {
                    ui.label(egui::RichText::new(tag).font(theme::medium(11.0)).color(t.text_faint));
                }
            });
            egui::Frame::NONE
                .fill(t.accent_soft)
                .corner_radius(CornerRadius { nw: 14, ne: 4, sw: 14, se: 14 })
                .inner_margin(egui::Margin::symmetric(12, 9))
                .show(ui, |ui| {
                    ui.set_max_width((ui.available_width() * 0.86).max(100.0));
                    ui.add(egui::Label::new(egui::RichText::new(&m.text).color(t.text)).wrap());
                });
        });
    });
}

fn source_tag(source: &Source) -> Option<String> {
    match source {
        Source::Document => None,
        Source::Page(p) => Some(crate::i18n::fmt(tl!("Page {n}"), &[("n", &p.saturating_add(1).to_string())])),
        Source::Text(_) => Some(tl!("Selection").to_string()),
    }
}

fn avatar(ui: &mut egui::Ui, t: &Tokens) {
    egui::Frame::NONE.fill(t.accent_soft).corner_radius(CornerRadius::same(9)).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
        ui.add(icons::image("sparkles", 12.0, t.accent_text));
    });
}

/// The answer flows as plain text across the whole width (no bubble), so tables and code get the
/// room they need; the name row and the footer frame it.
fn assistant(ui: &mut egui::Ui, t: &Tokens, m: &mut Msg, idx: usize, is_last: bool, running: bool, act: &mut Actions) {
    let streaming = running && is_last;
    ui.horizontal(|ui| {
        avatar(ui, t);
        ui.label(egui::RichText::new(tl!("Assistant")).font(theme::semibold(12.0)).color(t.text_muted));
    });
    ui.add_space(4.0);
    ui.scope(|ui| {
        ui.set_max_width(ui.available_width());
        if m.text.is_empty() {
            ui.label(egui::RichText::new("…").color(t.text_faint));
        } else {
            let blocks = m.blocks(streaming);
            if let Some(url) = crate::markdown::show(ui, &blocks, t, ui.scope_id().with(("pdf-ai-msg", idx))) {
                act.link = Some(url);
            }
        }
    });
    if !streaming && !m.text.is_empty() {
        ui.add_space(2.0);
        footer(ui, t, m, is_last, act);
    }
}

/// Copy, Regenerate (last answer only) and the model that wrote it.
fn footer(ui: &mut egui::Ui, t: &Tokens, m: &Msg, is_last: bool, act: &mut Actions) {
    ui.horizontal(|ui| {
        if icons::button(ui, "copy", 22.0, false, tl!("Copy answer")).clicked() {
            ui.ctx().copy_text(m.text.clone());
        }
        if is_last && icons::button(ui, "rotate-ccw", 22.0, false, tl!("Regenerate")).clicked() {
            act.again = true;
        }
        let mut note = m.model.clone().unwrap_or_default();
        if m.stopped {
            if !note.is_empty() {
                note.push_str(" · ");
            }
            note.push_str(tl!("Stopped"));
        }
        if !note.is_empty() {
            ui.label(egui::RichText::new(note).font(theme::regular(11.0)).color(t.text_faint));
        }
    });
}

fn error_bubble(ui: &mut egui::Ui, t: &Tokens, m: &Msg, can_retry: bool, act: &mut Actions) {
    let red = ui.visuals().error_fg_color;
    egui::Frame::NONE
        .fill(red.gamma_multiply(0.09))
        .stroke(Stroke::new(1.0, red.gamma_multiply(0.55)))
        .corner_radius(CornerRadius::same(11))
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.add(icons::image("triangle-alert", 15.0, red));
                ui.add(egui::Label::new(egui::RichText::new(&m.text).color(t.text)).wrap());
            });
            if can_retry {
                ui.add_space(6.0);
                if widgets::pill_button(ui, tl!("Retry"), false).clicked() {
                    act.again = true;
                }
            }
        });
}
