//! Read-only, conversational AI assistant for Pdf.
//!
//! The pane is a chat about the open document. The document is prepared locally once per
//! version and cached; nothing is sent until the user asks, and each turn resends the prepared
//! text plus the recent conversation to the user's installed Codex or Claude command. The
//! transcript lives in memory only; the pane never edits the PDF or stores credentials.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pdfcraft_ai::{AiError, Event, MarkdownDocument, Options, Request, Turn};

use crate::PdfCraftApp;
#[cfg(target_arch = "wasm32")]
use crate::{RightPanel, icons, theme::Tokens};

/// What the pane needs from the AI: preparing the document and answering. A seam so tests drive
/// the chat without running a real agent.
pub trait ChatBackend: Send + Sync {
    /// Turn PDF bytes into the text the agent reads.
    fn prepare(&self, pdf: &[u8]) -> Result<MarkdownDocument, AiError>;
    /// Answer `request` about `doc`; `sink` receives progress and text as it is written.
    fn converse(
        &self,
        doc: &MarkdownDocument,
        history: &[Turn],
        request: &Request,
        opts: &Options,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Event),
    ) -> Result<String, AiError>;
}

/// The real backend: the installed agent command, through `pdfcraft_ai`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PdfAiBackend;

impl ChatBackend for PdfAiBackend {
    fn prepare(&self, pdf: &[u8]) -> Result<MarkdownDocument, AiError> {
        pdfcraft_ai::prepare_pdf(pdf)
    }

    fn converse(
        &self,
        doc: &MarkdownDocument,
        history: &[Turn],
        request: &Request,
        opts: &Options,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Event),
    ) -> Result<String, AiError> {
        pdfcraft_ai::converse(doc, history, request, opts, cancel, sink)
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod composer;
#[cfg(not(target_arch = "wasm32"))]
mod header;
#[cfg(not(target_arch = "wasm32"))]
mod messages;
#[cfg(not(target_arch = "wasm32"))]
mod native;

#[cfg(not(target_arch = "wasm32"))]
pub use native::{AiPane, command, panel};

#[cfg(not(target_arch = "wasm32"))]
impl PdfCraftApp {
    /// Replace the AI backend (tests and automation); the panel uses the installed agents by default.
    pub fn set_ai_backend(&mut self, backend: Arc<dyn ChatBackend>) {
        self.ai.set_backend(backend);
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Default)]
pub struct AiPane;

#[cfg(target_arch = "wasm32")]
impl PdfCraftApp {
    /// The AI panel is a stub on the web: nothing to replace.
    pub fn set_ai_backend(&mut self, _backend: Arc<dyn ChatBackend>) {}
}

#[cfg(target_arch = "wasm32")]
pub fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    let (_, close) = crate::panels::panel_header(ui, &t, "AI assistant", None);
    ui.add(icons::image("sparkles", 18.0, t.accent_text));
    ui.label(tl!("The AI assistant is available in the desktop app."));
    if close {
        app.choose_right_panel(None);
    }
}

#[cfg(target_arch = "wasm32")]
pub fn command(app: &mut PdfCraftApp, _id: &str) {
    app.choose_right_panel(Some(RightPanel::Ai));
}
