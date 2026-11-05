//! Read-only, conversational AI assistant for Pdf.
//!
//! Every request starts from the current working PDF bytes and converts them locally with
//! MarkItDown. The user's installed Codex or Claude CLI then receives only that Markdown and the
//! selected reading task. The pane keeps a local, in-memory transcript; it never edits the PDF or
//! stores credentials.

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, RightPanel, icons};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::collections::VecDeque;
    use std::sync::Arc;
    use std::sync::mpsc::{Receiver, TryRecvError, channel};

    use egui::{Align, CornerRadius, Layout, Stroke};
    use pdfcraft_ai::{Provider, Request};

    use super::{PdfCraftApp, RightPanel, Tokens, icons, theme};

    const MAX_MESSAGES: usize = 40;

    struct Running(Receiver<Result<String, String>>);

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Role {
        User,
        Assistant,
        Error,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Message {
        role: Role,
        text: String,
    }

    pub struct AiPane {
        provider: Provider,
        language: String,
        question: String,
        messages: VecDeque<Message>,
        running: Option<Running>,
        queued: Option<Request>,
    }

    impl Default for AiPane {
        fn default() -> Self {
            Self {
                provider: Provider::Codex,
                language: "español".into(),
                question: String::new(),
                messages: VecDeque::new(),
                running: None,
                queued: None,
            }
        }
    }

    impl AiPane {
        fn push(&mut self, role: Role, text: String) {
            while self.messages.len() >= MAX_MESSAGES {
                self.messages.pop_front();
            }
            self.messages.push_back(Message { role, text });
        }

        fn start(&mut self, bytes: Arc<Vec<u8>>, request: Request, prompt: String, ctx: egui::Context) {
            if self.running.is_some() {
                return;
            }
            self.push(Role::User, prompt);
            let provider = self.provider;
            let (tx, rx) = channel();
            std::thread::spawn(move || {
                let result = pdfcraft_ai::analyse_pdf(&bytes, provider, &request);
                let _ = tx.send(result);
                ctx.request_repaint();
            });
            self.running = Some(Running(rx));
        }

        fn poll(&mut self) {
            let Some(running) = &self.running else { return };
            let result = match running.0.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => Some(Err("El asistente terminó inesperadamente.".into())),
                Err(TryRecvError::Empty) => None,
            };
            let Some(result) = result else { return };
            self.running = None;
            match result {
                Ok(answer) => self.push(Role::Assistant, answer),
                Err(error) => self.push(Role::Error, error),
            }
        }
    }

    fn request_prompt(request: &Request, spanish: bool) -> String {
        match request {
            Request::Summary if spanish => "Resume este PDF".into(),
            Request::Summary => "Summarize this PDF".into(),
            Request::KeyPoints if spanish => "Extrae los puntos clave".into(),
            Request::KeyPoints => "Extract the key points".into(),
            Request::Explain if spanish => "Explícamelo de forma sencilla".into(),
            Request::Explain => "Explain it simply".into(),
            Request::ActionItems if spanish => "Encuentra acciones, fechas y riesgos".into(),
            Request::ActionItems => "Find actions, dates, and risks".into(),
            Request::Translate { language } if spanish => format!("Traduce el PDF al {language}"),
            Request::Translate { language } => format!("Translate the PDF to {language}"),
            Request::Ask(question) => question.clone(),
        }
    }

    fn header(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, spanish: bool, close: &mut bool) {
        let say = |en: &'static str, es: &'static str| if spanish { es } else { en };
        ui.horizontal(|ui| {
            egui::Frame::NONE.fill(t.accent_soft).corner_radius(CornerRadius::same(9)).inner_margin(egui::Margin::same(7)).show(ui, |ui| {
                ui.add(icons::image("sparkles", 17.0, t.accent_text));
            });
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(tl!("AI assistant")).font(theme::semibold(15.5)));
                ui.label(egui::RichText::new(say("Chat with the current PDF", "Chatea con el PDF actual")).small().color(t.text_muted));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, "x", 26.0, false, tl!("Close")).clicked() {
                    *close = true;
                }
                let clear = icons::button(ui, "trash-2", 26.0, false, say("New chat", "Nuevo chat"));
                if clear.clicked() && app.ai.running.is_none() {
                    app.ai.messages.clear();
                }
            });
        });
        ui.add_space(9.0);
        ui.horizontal(|ui| {
            egui::Frame::NONE.fill(t.hover).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::symmetric(9, 4)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(icons::image("file-text", 13.0, t.text_muted));
                    ui.label(egui::RichText::new("MarkItDown · PDF").small().color(t.text_muted));
                });
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                craft_ui::ComboBox::from_id_salt("pdf-ai-provider").selected_text(app.ai.provider.id()).show_ui(ui, |ui| {
                    for provider in Provider::ALL {
                        ui.selectable_value(&mut app.ai.provider, provider, provider.id());
                    }
                });
                ui.label(egui::RichText::new(say("Agent", "Agente")).small().color(t.text_faint));
            });
        });
    }

    fn welcome(ui: &mut egui::Ui, t: &Tokens, spanish: bool) {
        let say = |en: &'static str, es: &'static str| if spanish { es } else { en };
        ui.add_space(10.0);
        egui::Frame::NONE
            .fill(t.hover)
            .stroke(Stroke::new(1.0, t.divider))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(egui::Margin::symmetric(14, 16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    ui.add(icons::image("sparkles", 24.0, t.accent_text));
                    ui.add_space(7.0);
                    ui.label(egui::RichText::new(say("What would you like to know?", "¿Qué quieres saber?")).font(theme::semibold(15.0)));
                    ui.label(
                        egui::RichText::new(say(
                            "Ask a question or choose a suggestion below. Pdf prepares the document locally with MarkItDown first.",
                            "Haz una pregunta o elige una sugerencia. Pdf prepara antes el documento localmente con MarkItDown.",
                        ))
                        .small()
                        .color(t.text_muted),
                    );
                });
            });
    }

    fn assistant_message(ui: &mut egui::Ui, t: &Tokens, text: &str, error: bool, spanish: bool) {
        let label = if spanish { "Asistente de Pdf" } else { "Pdf assistant" };
        ui.horizontal(|ui| {
            ui.set_max_width(ui.available_width());
            egui::Frame::NONE.fill(t.accent_soft).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                ui.add(icons::image("sparkles", 15.0, t.accent_text));
            });
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).font(theme::medium(11.5)).color(t.text_faint));
                egui::Frame::NONE
                    .fill(t.card)
                    .stroke(Stroke::new(1.0, if error { egui::Color32::from_rgb(0xC4, 0x2B, 0x1C) } else { t.border }))
                    .corner_radius(CornerRadius { nw: 3, ne: 10, sw: 10, se: 10 })
                    .inner_margin(egui::Margin::symmetric(11, 9))
                    .show(ui, |ui| {
                        ui.set_max_width((ui.available_width() * 0.94).max(100.0));
                        ui.add(
                            egui::Label::new(egui::RichText::new(text).color(if error { egui::Color32::from_rgb(0xC4, 0x2B, 0x1C) } else { t.text }))
                                .wrap(),
                        );
                    });
            });
        });
    }

    fn user_message(ui: &mut egui::Ui, t: &Tokens, text: &str, spanish: bool) {
        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
            ui.vertical(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(egui::RichText::new(if spanish { "Tú" } else { "You" }).font(theme::medium(11.5)).color(t.text_faint));
                });
                egui::Frame::NONE
                    .fill(t.accent_soft)
                    .corner_radius(CornerRadius { nw: 10, ne: 3, sw: 10, se: 10 })
                    .inner_margin(egui::Margin::symmetric(11, 9))
                    .show(ui, |ui| {
                        ui.set_max_width((ui.available_width() * 0.82).max(100.0));
                        ui.add(egui::Label::new(egui::RichText::new(text).color(t.text)).wrap());
                    });
            });
        });
    }

    fn conversation(app: &PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, spanish: bool, height: f32) {
        egui::ScrollArea::vertical().id_salt("pdf-ai-conversation").auto_shrink([false, false]).stick_to_bottom(true).max_height(height).show(
            ui,
            |ui| {
                ui.set_width(ui.available_width());
                ui.set_min_height(height);
                if app.ai.messages.is_empty() {
                    welcome(ui, t, spanish);
                } else {
                    ui.add_space(5.0);
                    for message in &app.ai.messages {
                        match message.role {
                            Role::User => user_message(ui, t, &message.text, spanish),
                            Role::Assistant => assistant_message(ui, t, &message.text, false, spanish),
                            Role::Error => assistant_message(ui, t, &message.text, true, spanish),
                        }
                        ui.add_space(12.0);
                    }
                }
                if app.ai.running.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new(if spanish {
                                "Preparando con MarkItDown y pensando…"
                            } else {
                                "Preparing with MarkItDown and thinking…"
                            })
                            .small()
                            .color(t.text_muted),
                        );
                    });
                    ui.add_space(8.0);
                }
            },
        );
    }

    fn suggestions(app: &mut PdfCraftApp, ui: &mut egui::Ui, spanish: bool) -> Option<Request> {
        let say = |en: &'static str, es: &'static str| if spanish { es } else { en };
        let mut request = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            if crate::widgets::pill_button(ui, tl!("Summarize"), false).clicked() {
                request = Some(Request::Summary);
            }
            if crate::widgets::pill_button(ui, say("Key points", "Puntos clave"), false).clicked() {
                request = Some(Request::KeyPoints);
            }
            if crate::widgets::pill_button(ui, say("Explain", "Explicar"), false).clicked() {
                request = Some(Request::Explain);
            }
            if crate::widgets::pill_button(ui, say("Actions & risks", "Acciones y riesgos"), false).clicked() {
                request = Some(Request::ActionItems);
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add(icons::image("languages", 15.0, Tokens::get(ui.ctx()).text_muted));
            ui.label(egui::RichText::new(say("Translate to", "Traducir a")).small());
            ui.add(crate::widgets::line(&mut app.ai.language).desired_width(82.0).hint_text(say("language", "idioma")));
            if crate::widgets::pill_button(ui, say("Translate", "Traducir"), false).clicked() {
                let language = app.ai.language.trim().to_string();
                if !language.is_empty() {
                    request = Some(Request::Translate { language });
                }
            }
        });
        request
    }

    fn composer(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, spanish: bool) -> bool {
        let say = |en: &'static str, es: &'static str| if spanish { es } else { en };
        let mut send = false;
        egui::Frame::NONE
            .fill(t.field)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(CornerRadius::same(11))
            .inner_margin(egui::Margin { left: 10, right: 7, top: 8, bottom: 6 })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let input = ui.add(
                    egui::TextEdit::multiline(&mut app.ai.question)
                        .desired_rows(2)
                        .desired_width(f32::INFINITY)
                        .frame(egui::Frame::NONE)
                        .hint_text(say("Ask anything about this PDF…", "Pregunta lo que quieras sobre este PDF…"))
                        .id_salt("pdf-ai-chat-input"),
                );
                let shortcut = input.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(say("Ctrl/⌘ + Enter to send", "Ctrl/⌘ + Intro para enviar")).small().color(t.text_faint));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let enabled = app.ai.running.is_none() && !app.ai.question.trim().is_empty();
                        ui.add_enabled_ui(enabled, |ui| {
                            if icons::button(ui, "send", 30.0, true, say("Send", "Enviar")).clicked() {
                                send = true;
                            }
                        });
                    });
                });
                send |= shortcut;
            });
        send
    }

    pub fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
        app.ai.poll();
        let t = Tokens::get(ui.ctx());
        let bytes = app.active_ids().and_then(|(_, id)| app.session.get(id)).map(|doc| doc.bytes.clone());
        let spanish = app.language == "es" || (app.language == crate::i18n::AUTO && crate::i18n::current().code() == "es");
        let mut close = false;
        let mut request = app.ai.queued.take();
        egui::Frame::NONE
            .fill(t.panel)
            .inner_margin(egui::Margin { left: 12, right: 10, top: 11, bottom: 9 })
            .stroke(Stroke::new(1.0, t.divider))
            .show(ui, |ui| {
                header(app, ui, &t, spanish, &mut close);
                ui.add_space(8.0);
                ui.separator();
                let conversation_height = (ui.available_height() - 270.0).max(150.0);
                conversation(app, ui, &t, spanish, conversation_height);
                ui.separator();
                ui.add_space(7.0);
                let enabled = bytes.is_some() && app.ai.running.is_none();
                ui.add_enabled_ui(enabled, |ui| {
                    request = suggestions(app, ui, spanish).or(request.take());
                    ui.add_space(8.0);
                    if composer(app, ui, &t, spanish) && !app.ai.question.trim().is_empty() {
                        request = Some(Request::Ask(app.ai.question.trim().to_string()));
                    }
                });
                ui.add_space(5.0);
                ui.horizontal(|ui| {
                    ui.add(icons::image("shield-check", 12.0, t.text_faint));
                    ui.label(
                        egui::RichText::new(if spanish {
                            "El PDF no se envía hasta que tú preguntes."
                        } else {
                            "The PDF is not sent until you ask."
                        })
                        .small()
                        .color(t.text_faint),
                    );
                });
            });
        if close {
            app.choose_right_panel(None);
        }
        if let (Some(bytes), Some(request)) = (bytes, request) {
            let prompt = request_prompt(&request, spanish);
            let asked = matches!(&request, Request::Ask(_));
            app.ai.start(bytes, request, prompt, ui.ctx().clone());
            if asked {
                app.ai.question.clear();
            }
        }
    }

    pub fn command(app: &mut PdfCraftApp, id: &str) {
        app.choose_right_panel(Some(RightPanel::Ai));
        app.ai.queued = match id {
            "ai.summary" => Some(Request::Summary),
            "ai.translate" => Some(Request::Translate {
                language: match app.ai.language.trim() {
                    "" => "español".into(),
                    language => language.into(),
                },
            }),
            _ => None,
        };
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn pane_defaults_to_an_empty_codex_chat() {
            let pane = AiPane::default();
            assert_eq!(pane.provider, Provider::Codex);
            assert_eq!(pane.language, "español");
            assert!(pane.messages.is_empty());
            assert!(pane.running.is_none());
        }

        #[test]
        fn transcript_is_bounded_to_recent_messages() {
            let mut pane = AiPane::default();
            for n in 0..MAX_MESSAGES + 4 {
                pane.push(Role::User, n.to_string());
            }
            assert_eq!(pane.messages.len(), MAX_MESSAGES);
            assert_eq!(pane.messages.front().map(|message| message.text.as_str()), Some("4"));
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{AiPane, command, panel};

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct AiPane;

#[cfg(target_arch = "wasm32")]
pub fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (_, close) = crate::panels::panel_header(ui, &t, "AI assistant", None);
    ui.label(tl!("The AI assistant is available in the desktop app."));
    if close {
        app.choose_right_panel(None);
    }
}

#[cfg(target_arch = "wasm32")]
pub fn command(app: &mut PdfCraftApp, _id: &str) {
    app.choose_right_panel(Some(RightPanel::Ai));
}
