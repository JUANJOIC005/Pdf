//! The AI assistant panel as a chat, driven through a fake backend so no agent runs: the answer is
//! rendered as Markdown, Stop cancels, the agent and model choice is saved, and the panel never
//! names the tool that prepares the document.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_ai::{AiError, Event, MarkdownDocument, ModelChoice, Options, Phase, Provider, Request, Turn};
use pdfcraft_engine::Session;
use pdfcraft_ui_egui::{ChatBackend, PdfCraftApp};

use crate::support;

const TABLE: &str = "Here it is:\n\n| Name | Qty |\n|:-----|----:|\n| Alpha | 12 |\n| Beta | 7 |\n";

/// Answers with `TABLE`, or waits for the cancel flag when `block` is set.
struct Fake {
    block: bool,
    cancelled: Arc<AtomicBool>,
    asked: std::sync::Mutex<Vec<(Vec<Turn>, Request)>>,
}

impl ChatBackend for Fake {
    fn prepare(&self, _pdf: &[u8]) -> Result<MarkdownDocument, AiError> {
        Ok(MarkdownDocument { text: "doc".into(), truncated: false })
    }

    fn converse(
        &self,
        _doc: &MarkdownDocument,
        history: &[Turn],
        request: &Request,
        _opts: &Options,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Event),
    ) -> Result<String, AiError> {
        self.asked.lock().unwrap().push((history.to_vec(), request.clone()));
        sink(Event::Phase(Phase::Thinking));
        sink(Event::Model("fake-1".into()));
        if self.block {
            while !cancel.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            self.cancelled.store(true, Ordering::SeqCst);
            return Err(AiError::Cancelled);
        }
        sink(Event::Phase(Phase::Writing));
        sink(Event::Delta(TABLE.into()));
        Ok(TABLE.into())
    }
}

fn fake(block: bool) -> Arc<Fake> {
    Arc::new(Fake { block, cancelled: Arc::new(AtomicBool::new(false)), asked: Default::default() })
}

fn harness(backend: Arc<Fake>) -> Harness<'static, PdfCraftApp> {
    let pdf = Session::new().create_from_text("t", "Delivery within five days.").unwrap().to_vec();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("t.pdf", None, pdf.clone()).unwrap();
        app.set_ai_backend(backend.clone());
        app.set_option("panel", "ai").unwrap();
        app
    });
    h.run_steps(6);
    h
}

fn ask_summary(h: &mut Harness<'static, PdfCraftApp>) {
    h.get_by_label("Summarize").click();
    h.run_steps(2);
}

#[test]
fn an_answer_is_rendered_as_markdown_with_its_model_and_actions() {
    let mut h = harness(fake(false));
    ask_summary(&mut h);
    assert!(support::step_until(&mut h, |h| h.query_by_label("Alpha").is_some()), "the table cell is in the tree");
    h.get_by_label("Beta");
    h.get_by_label("fake-1");
    h.get_by_label("Copy answer");
    h.get_by_label("Regenerate");
    h.get_by_label("Summarize this document");
}

#[test]
fn stop_appears_while_answering_and_cancels() {
    let backend = fake(true);
    let mut h = harness(backend.clone());
    ask_summary(&mut h);
    assert!(support::step_until(&mut h, |h| h.query_by_label("Stop").is_some()), "Stop replaces Send");
    assert!(h.query_by_label("Send").is_none());
    h.get_by_label("Thinking…");
    h.get_by_label("Stop").click();
    assert!(support::step_until(&mut h, |h| h.query_by_label("Stop").is_none()), "the chat is idle again");
    assert!(backend.cancelled.load(Ordering::SeqCst), "the backend saw the cancel flag");
    h.get_by_label("Send");
}

#[test]
fn escape_also_stops() {
    let backend = fake(true);
    let mut h = harness(backend.clone());
    ask_summary(&mut h);
    assert!(support::step_until(&mut h, |h| h.query_by_label("Stop").is_some()));
    h.key_press(egui::Key::Escape);
    assert!(support::step_until(&mut h, |h| h.query_by_label("Stop").is_none()));
    assert!(backend.cancelled.load(Ordering::SeqCst));
}

#[test]
fn enter_sends_the_question_and_the_next_turn_carries_the_history() {
    let backend = fake(false);
    let mut h = harness(backend.clone());
    let input = h.get_by_role(egui::accesskit::Role::MultilineTextInput);
    input.focus();
    input.type_text("What is the delivery time?");
    h.run_steps(2);
    h.key_press(egui::Key::Enter);
    assert!(support::step_until(&mut h, |h| h.query_by_label("Alpha").is_some()));
    h.get_by_label("What is the delivery time?");
    ask_summary(&mut h);
    assert!(support::step_until(&mut h, |_| backend.asked.lock().unwrap().len() == 2));
    let asked = backend.asked.lock().unwrap();
    assert_eq!(asked[0].1, Request::Ask("What is the delivery time?".into()));
    assert_eq!(asked[1].0.len(), 2, "the second turn resends the first question and its answer");
}

fn all_text(h: &Harness<'static, PdfCraftApp>) -> String {
    fn walk(node: &egui_kittest::Node<'_>, out: &mut String) {
        let n = node.accesskit_node();
        for s in [n.label(), n.value()].into_iter().flatten() {
            out.push_str(&s);
            out.push('\n');
        }
        for c in node.children() {
            walk(&c, out);
        }
    }
    let mut out = String::new();
    walk(&h.root(), &mut out);
    out
}

#[test]
fn the_panel_never_names_the_preparing_tool() {
    let mut h = harness(fake(false));
    assert!(!all_text(&h).to_lowercase().contains("markitdown"));
    ask_summary(&mut h);
    assert!(support::step_until(&mut h, |h| h.query_by_label("Alpha").is_some()));
    let text = all_text(&h);
    assert!(text.contains("Regenerate"), "the walk reaches the chat");
    assert!(!text.to_lowercase().contains("markitdown"));
}

#[test]
fn choosing_an_agent_and_model_is_saved_and_restored() {
    let mut h = harness(fake(false));
    h.get_by_label("Agent").click();
    h.run_steps(2);
    h.get_by_label("Claude").click();
    h.run_steps(2);
    h.get_by_label("Model").click();
    h.run_steps(2);
    h.get_by_label("opus").click();
    h.run_steps(2);
    let c = h.state().ai_choice.clone();
    assert_eq!(c.provider, Provider::Claude);
    assert_eq!(c.model_claude, ModelChoice::Named("opus".into()));
    assert_eq!(c.model_codex, ModelChoice::Auto);
    let mut restored = PdfCraftApp::new();
    restored.restore(&h.state().persist());
    assert_eq!(restored.ai_choice, c);
}
