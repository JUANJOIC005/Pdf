//! The panel's header: title, agent and model, and the conversation menu.

use egui::{Align, CornerRadius, Layout, Stroke};

use super::native::Actions;
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons};

pub(super) fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, act: &mut Actions) {
    ui.horizontal(|ui| {
        egui::Frame::NONE.fill(t.accent_soft).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.add(icons::image("sparkles", 17.0, t.accent_text));
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(tl!("AI assistant")).font(theme::semibold(15.5)).color(t.text));
            ui.label(egui::RichText::new(tl!("Chat with the current PDF")).font(theme::regular(11.5)).color(t.text_muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icons::button(ui, "x", 26.0, false, tl!("Close")).clicked() {
                act.close = true;
            }
            conversation_menu(app, ui, act);
        });
    });
    ui.add_space(8.0);
    egui::Frame::NONE
        .fill(t.hover)
        .stroke(Stroke::new(1.0, t.divider))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(9, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            crate::ai_model::picker(ui, &mut app.ai_choice, t);
        });
}

/// The "⋯" menu: start over or export.
fn conversation_menu(app: &PdfCraftApp, ui: &mut egui::Ui, act: &mut Actions) {
    let more = icons::button(ui, "ellipsis", 26.0, false, tl!("Conversation options"));
    egui::Popup::menu(&more).show(|ui| {
        ui.set_min_width(200.0);
        let idle = !app.ai.running();
        if ui.add_enabled(idle && !app.ai.messages.is_empty(), egui::Button::new(tl!("New conversation"))).clicked() {
            act.new_chat = true;
            ui.close();
        }
        if ui.add_enabled(!app.ai.messages.is_empty(), egui::Button::new(tl!("Export to Markdown…"))).clicked() {
            act.export = true;
            ui.close();
        }
    });
}
