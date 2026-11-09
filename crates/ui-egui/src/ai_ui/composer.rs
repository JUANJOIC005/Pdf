//! Everything under the conversation: status, quick actions, the input card and the privacy note.
//!
//! The group is laid out top-down inside one block that the panel pins to the bottom, so wrapped
//! rows (the quick actions) take the height they need instead of overlapping the rows below.

use egui::{Align, Color32, CornerRadius, Key, Layout, Sense, Stroke, vec2};
use pdfcraft_ai::{Phase, Request};

use super::native::{Actions, ContextChoice};
use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

const INPUT_ID: &str = "pdf-ai-chat-input";

pub(super) fn show(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, has_doc: bool, act: &mut Actions) {
    let running = app.ai.running();
    let can_ask = has_doc && !running;
    {
        status(app, ui, t);
        if !app.ai.messages.is_empty() {
            ui.add_enabled_ui(can_ask, |ui| {
                if let Some(request) = chips(ui) {
                    act.send = Some(request);
                }
            });
            ui.add_space(6.0);
        }
        input_card(app, ui, t, has_doc, can_ask, act);
        ui.add_space(6.0);
        privacy(ui, t);
    }
}

fn privacy(ui: &mut egui::Ui, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.add(icons::image("shield-check", 12.0, t.text_faint));
        ui.label(egui::RichText::new(tl!("The document is not sent until you ask.")).font(theme::regular(11.0)).color(t.text_faint));
    });
}

/// The quick actions as compact pills (under a conversation). Returns the one clicked.
pub(super) fn chips(ui: &mut egui::Ui) -> Option<Request> {
    let mut request = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (label, req) in actions() {
            if widgets::pill_button(ui, label, false).clicked() {
                request = Some(req);
            }
        }
    });
    request
}

fn actions() -> [(&'static str, Request); 4] {
    [
        (tl!("Summarize"), Request::Summary),
        (tl!("Key points"), Request::KeyPoints),
        (tl!("Explain"), Request::Explain),
        (tl!("Actions & risks"), Request::ActionItems),
    ]
}

/// The quick actions as full-width rows with an icon (the welcome screen). Returns the one clicked.
pub(super) fn suggestions(ui: &mut egui::Ui, t: &Tokens, enabled: bool) -> Option<Request> {
    let mut request = None;
    let icons_for = ["file-text", "list-checks", "wand-sparkles", "triangle-alert"];
    for ((label, req), icon) in actions().into_iter().zip(icons_for) {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), if enabled { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
        let press = theme::Press::track(ui, &resp);
        let fill = if enabled { press.fill(t.card, t.hover, t.pressed) } else { t.card };
        let ink = if enabled { t.text } else { t.text_faint };
        ui.painter().rect(rect, CornerRadius::same(11), fill, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        let tile = egui::Rect::from_center_size(rect.left_center() + vec2(22.0, 0.0), vec2(26.0, 26.0));
        ui.painter().rect_filled(tile, CornerRadius::same(8), if enabled { t.accent_soft } else { t.hover });
        icons::paint(ui, tile, icon, 15.0, if enabled { t.accent_text } else { t.text_faint });
        ui.painter().text(rect.left_center() + vec2(44.0, 0.0), egui::Align2::LEFT_CENTER, label, theme::medium(13.0), ink);
        if enabled && resp.clicked() {
            request = Some(req);
        }
        if enabled {
            theme::hand(resp);
        }
        ui.add_space(6.0);
    }
    request
}

/// "Whole document / Current page / Selection".
fn context_row(app: &mut PdfCraftApp, ui: &mut egui::Ui, enabled: bool) {
    let has_selection = app.active_ids().and_then(|(i, _)| app.views.get(i)).is_some_and(|v| v.has_text_selection());
    let label = |c: ContextChoice| match c {
        ContextChoice::Document => tl!("Entire document"),
        ContextChoice::Page => tl!("Current page"),
        ContextChoice::Selection => tl!("Selection"),
    };
    ui.add_enabled_ui(enabled, |ui| {
        ui.add(icons::image("file-text", 13.0, Tokens::get(ui.ctx()).text_muted));
        let shown = label(app.ai.context);
        let combo = craft_ui::ComboBox::from_id_salt("pdf-ai-context").selected_text(shown).show_ui(ui, |ui| {
            for c in [ContextChoice::Document, ContextChoice::Page, ContextChoice::Selection] {
                let ok = c != ContextChoice::Selection || has_selection || app.ai.context == c;
                ui.add_enabled_ui(ok, |ui| ui.selectable_value(&mut app.ai.context, c, label(c)));
            }
        });
        combo.response.widget_info(|| {
            let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, enabled, tl!("Context"));
            info.current_text_value = Some(shown.to_string());
            info
        });
    });
}

fn status(app: &PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    if let Some(run) = &app.ai.run {
        ui.horizontal(|ui| {
            ui.spinner();
            let text = match run.phase {
                Phase::Preparing => tl!("Preparing the document…"),
                Phase::Thinking => tl!("Thinking…"),
                Phase::Writing => tl!("Writing…"),
            };
            ui.label(egui::RichText::new(text).small().color(t.text_muted));
        });
        ui.add_space(6.0);
    } else if let Some(hint) = &app.ai.hint {
        ui.label(egui::RichText::new(hint).small().color(ui.visuals().error_fg_color));
        ui.add_space(6.0);
    }
}

/// Enter sends and Shift+Enter inserts a line break: the key is taken before the field sees it.
fn enter_pressed(ui: &egui::Ui, focused: bool) -> bool {
    focused
        && ui.input_mut(|i| {
            let mods = i.modifiers;
            i.key_pressed(Key::Enter) && !mods.shift && i.consume_key(mods, Key::Enter)
        })
}

/// The round send/stop button. `label` is what assistive tech and the tooltip say.
fn round_button(ui: &mut egui::Ui, t: &Tokens, icon: &str, label: &str, tip: &str, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(32.0, 32.0), if active { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, active, label));
    let press = theme::Press::track(ui, &resp);
    let fill = if active { press.fill(t.accent, t.accent_text, t.accent.lerp_to_gamma(Color32::BLACK, 0.22)) } else { t.pressed };
    ui.painter().circle_filled(rect.center(), 16.0, fill);
    icons::paint(ui, rect, icon, 16.0, if active { Color32::WHITE } else { t.text_faint });
    let resp = resp.on_hover_text(tip);
    if active { theme::hand(resp) } else { resp }
}

fn input_card(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, has_doc: bool, can_ask: bool, act: &mut Actions) {
    let id = ui.scope_id().with(INPUT_ID);
    let running = app.ai.running();
    let focused = ui.memory(|m| m.has_focus(id));
    let enter = enter_pressed(ui, focused);
    // Esc stops an answer while the pointer is over the panel or the field has the focus.
    if running && (focused || act.pointer_over) && ui.input(|i| i.key_pressed(Key::Escape)) {
        act.stop = true;
    }
    let mut send = false;
    egui::Frame::NONE
        .fill(t.field)
        .stroke(Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { t.accent } else { t.border }))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(egui::Margin { left: 12, right: 8, top: 10, bottom: 8 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_enabled(
                has_doc,
                egui::TextEdit::multiline(&mut app.ai.input)
                    .id(id)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .frame(egui::Frame::NONE)
                    .hint_text(tl!("Ask anything about this document…")),
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                context_row(app, ui, can_ask);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let tip = tl!("Enter to send · Shift+Enter for a new line");
                    if running {
                        if round_button(ui, t, "square", tl!("Stop"), tl!("Stop"), true).clicked() {
                            act.stop = true;
                        }
                    } else {
                        let ready = has_doc && !app.ai.input.trim().is_empty();
                        send = round_button(ui, t, "send", tl!("Send"), tip, ready).clicked();
                    }
                });
            });
        });
    if (send || enter) && !running && has_doc && !app.ai.input.trim().is_empty() {
        act.send = Some(Request::Ask(app.ai.input.trim().to_string()));
    }
}
