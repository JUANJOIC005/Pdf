//! Small, read-only AI assistant for the Pdf fork.
//!
//! Every request starts from the current working PDF bytes and converts them locally with
//! MarkItDown. The user's installed Codex or Claude CLI then receives only that Markdown and the
//! selected reading task. The pane never edits the PDF or stores credentials.

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, RightPanel, icons};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::Arc;
    use std::sync::mpsc::{Receiver, TryRecvError, channel};

    use egui::{Align, Layout};
    use pdfcraft_ai::{Provider, Request};

    use super::*;

    struct Running(Receiver<Result<String, String>>);

    pub struct AiPane {
        provider: Provider,
        language: String,
        question: String,
        answer: String,
        error: Option<String>,
        running: Option<Running>,
        queued: Option<Request>,
    }

    impl Default for AiPane {
        fn default() -> Self {
            Self {
                provider: Provider::Codex,
                language: "español".into(),
                question: String::new(),
                answer: String::new(),
                error: None,
                running: None,
                queued: None,
            }
        }
    }

    impl AiPane {
        fn start(&mut self, bytes: Arc<Vec<u8>>, request: Request, ctx: egui::Context) {
            if self.running.is_some() {
                return;
            }
            self.answer.clear();
            self.error = None;
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
            match running.0.try_recv() {
                Ok(Ok(answer)) => {
                    self.answer = answer;
                    self.running = None;
                }
                Ok(Err(error)) => {
                    self.error = Some(error);
                    self.running = None;
                }
                Err(TryRecvError::Disconnected) => {
                    self.error = Some("El asistente terminó inesperadamente.".into());
                    self.running = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    pub fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
        app.ai.poll();
        let t = Tokens::get(ui.ctx());
        let bytes = app.active_ids().and_then(|(_, id)| app.session.get(id)).map(|doc| doc.bytes.clone());
        let spanish = app.language == "es" || (app.language == crate::i18n::AUTO && crate::i18n::current().code() == "es");
        let say = |en: &'static str, es: &'static str| if spanish { es } else { en };
        let mut close = false;
        let mut request = app.ai.queued.take();
        egui::Frame::NONE
            .fill(t.panel)
            .inner_margin(egui::Margin { left: 14, right: 12, top: 12, bottom: 8 })
            .stroke(egui::Stroke::new(1.0, t.divider))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(icons::image("sparkles", 18.0, t.accent));
                    ui.label(egui::RichText::new(tl!("AI assistant")).font(theme::semibold(15.5)));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if icons::button(ui, "x", 26.0, false, tl!("Close")).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(say(
                        "The current PDF is converted locally with MarkItDown for every request.",
                        "El PDF actual se convierte localmente con MarkItDown en cada petición.",
                    ))
                    .small()
                    .color(t.text_muted),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(say("Agent", "Agente"));
                    egui::ComboBox::from_id_salt("pdf-ai-provider").selected_text(app.ai.provider.id()).show_ui(ui, |ui| {
                        for provider in Provider::ALL {
                            ui.selectable_value(&mut app.ai.provider, provider, provider.id());
                        }
                    });
                });
                ui.add_space(6.0);
                let enabled = bytes.is_some() && app.ai.running.is_none();
                ui.add_enabled_ui(enabled, |ui| {
                    ui.columns(2, |columns| {
                        if columns[0].button(tl!("Summarize")).clicked() {
                            request = Some(Request::Summary);
                        }
                        if columns[1].button(say("Key points", "Puntos clave")).clicked() {
                            request = Some(Request::KeyPoints);
                        }
                        if columns[0].button(say("Explain simply", "Explicar fácil")).clicked() {
                            request = Some(Request::Explain);
                        }
                        if columns[1].button(say("Actions and risks", "Acciones y riesgos")).clicked() {
                            request = Some(Request::ActionItems);
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(tl!("Translate"));
                        ui.add(egui::TextEdit::singleline(&mut app.ai.language).desired_width(100.0).hint_text(say("language", "idioma")));
                        if ui.button(say("Translate", "Traducir")).clicked() {
                            let language = app.ai.language.trim().to_string();
                            if !language.is_empty() {
                                request = Some(Request::Translate { language });
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(say("Ask about this PDF", "Pregunta sobre este PDF"));
                    ui.add(
                        egui::TextEdit::multiline(&mut app.ai.question)
                            .desired_rows(2)
                            .hint_text(say("What do you want to know?", "¿Qué quieres saber?")),
                    );
                    if ui.button(say("Ask", "Preguntar")).clicked() && !app.ai.question.trim().is_empty() {
                        request = Some(Request::Ask(app.ai.question.trim().to_string()));
                    }
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
                if app.ai.running.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(say("Converting with MarkItDown and analysing…", "Convirtiendo con MarkItDown y analizando…"));
                    });
                } else if let Some(error) = &app.ai.error {
                    ui.colored_label(egui::Color32::from_rgb(0xC4, 0x2B, 0x1C), error);
                } else if app.ai.answer.is_empty() {
                    ui.label(
                        egui::RichText::new(say(
                            "Choose an action. The PDF is not sent anywhere until you do.",
                            "Elige una acción. El PDF no se envía a ningún agente hasta entonces.",
                        ))
                        .color(t.text_faint),
                    );
                } else {
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        ui.add(egui::Label::new(egui::RichText::new(&app.ai.answer).color(t.text)).wrap());
                    });
                }
            });
        if close {
            app.choose_right_panel(None);
        }
        if let (Some(bytes), Some(request)) = (bytes, request) {
            app.ai.start(bytes, request, ui.ctx().clone());
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
        fn pane_defaults_to_codex_and_spanish_translation() {
            let pane = AiPane::default();
            assert_eq!(pane.provider, Provider::Codex);
            assert_eq!(pane.language, "español");
            assert!(pane.running.is_none());
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
