//! Chat state, the worker thread and the panel's frame. The pieces are in `header`, `messages`
//! and `composer`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use egui::Stroke;
use pdfcraft_ai::{AiError, Event, MarkdownDocument, Phase, Request, Role, Turn};

use super::{ChatBackend, PdfAiBackend, composer, header, messages};
use crate::markdown::MarkdownCache;
use crate::theme::Tokens;
use crate::{PdfCraftApp, RightPanel};

/// Older messages are dropped past this many.
const MAX_MESSAGES: usize = 200;
/// How often a streaming answer is parsed again at most (a line break parses at once).
pub(super) const REPARSE: Duration = Duration::from_millis(60);

/// What the question is about.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ContextChoice {
    #[default]
    Document,
    Page,
    Selection,
}

/// The text a request is about, resolved when it is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Document,
    Page(usize),
    Text(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Who {
    User,
    Assistant,
    Error,
}

pub(super) struct Msg {
    pub who: Who,
    pub text: String,
    /// The model the agent reported for this answer.
    pub model: Option<String>,
    /// For a user message: what to ask again on Retry or Regenerate.
    pub ask: Option<(Request, Source)>,
    /// The answer was cut short by Stop.
    pub stopped: bool,
    pub cache: MarkdownCache,
    /// How much of `text` the last parse covered while streaming.
    snap_len: usize,
    last_parse: Option<Instant>,
    force_parse: bool,
    got_delta: bool,
}

impl Msg {
    fn new(who: Who, text: String) -> Self {
        Self { who, text, model: None, ask: None, stopped: false, cache: MarkdownCache::default(), snap_len: 0, last_parse: None, force_parse: false, got_delta: false }
    }

    /// The parsed text. While streaming it is parsed again at most every [`REPARSE`], or at once
    /// after a line break, so a long answer is not parsed on every chunk.
    pub fn blocks(&mut self, streaming: bool) -> Arc<Vec<crate::markdown::Block>> {
        let now = Instant::now();
        let due = self.last_parse.is_none_or(|t| now.duration_since(t) >= REPARSE);
        if !streaming {
            self.snap_len = self.text.len();
        } else if self.force_parse || due {
            self.snap_len = self.text.len();
            self.last_parse = Some(now);
            self.force_parse = false;
        }
        // `snap_len` was taken from `text` when it was whole and `text` only grows by whole
        // chunks, so it is a char boundary; `get` falls back to the whole text if not.
        let shown = self.text.get(..self.snap_len).unwrap_or(&self.text);
        self.cache.get(shown)
    }
}

enum Work {
    Prepared(u64, Arc<MarkdownDocument>),
    Event(Event),
    Done(Result<String, AiError>),
}

pub(super) struct Run {
    rx: Receiver<Work>,
    pub cancel: Arc<AtomicBool>,
    pub phase: Phase,
}

pub struct AiPane {
    backend: Arc<dyn ChatBackend>,
    pub(super) messages: Vec<Msg>,
    pub(super) input: String,
    pub(super) context: ContextChoice,
    pub(super) run: Option<Run>,
    /// A request from a command (Translate document), sent when the panel next shows.
    queued: Option<Request>,
    /// The language the `ai.translate` command asks for.
    translate_language: String,
    /// The prepared document and the hash of the bytes it came from.
    prepared: Option<(u64, Arc<MarkdownDocument>)>,
    /// A line under the chat, e.g. why a request was not sent.
    pub(super) hint: Option<String>,
}

impl Default for AiPane {
    fn default() -> Self {
        Self {
            backend: Arc::new(PdfAiBackend),
            messages: Vec::new(),
            input: String::new(),
            context: ContextChoice::default(),
            run: None,
            queued: None,
            translate_language: "español".into(),
            prepared: None,
            hint: None,
        }
    }
}

fn bytes_key(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// The text of one page, as the renderer extracts it.
fn page_text(bytes: &Arc<Vec<u8>>, page: usize) -> String {
    let mut r = pdfcraft_render::PageRenderer::new(bytes.clone(), pdfcraft_render::RenderConfig::default());
    let request = pdfcraft_render::RenderRequest { page, kind: pdfcraft_render::RequestKind::Text, scale: 1.0, ..Default::default() };
    r.render(request).text.map(|t| t.plain_text()).unwrap_or_default()
}

struct Job {
    backend: Arc<dyn ChatBackend>,
    bytes: Arc<Vec<u8>>,
    source: Source,
    cached: Option<(u64, Arc<MarkdownDocument>)>,
    history: Vec<Turn>,
    request: Request,
    opts: pdfcraft_ai::Options,
    cancel: Arc<AtomicBool>,
}

impl Job {
    fn document(&self, tx: &Sender<Work>) -> Result<Arc<MarkdownDocument>, AiError> {
        match &self.source {
            Source::Text(text) => Ok(Arc::new(MarkdownDocument { text: text.clone(), truncated: false })),
            Source::Page(page) => {
                let text = page_text(&self.bytes, *page);
                if text.trim().is_empty() {
                    return Err(AiError::NoText);
                }
                Ok(Arc::new(MarkdownDocument { text, truncated: false }))
            }
            Source::Document => {
                let key = bytes_key(&self.bytes);
                if let Some((k, doc)) = &self.cached
                    && *k == key
                {
                    return Ok(Arc::clone(doc));
                }
                let _ = tx.send(Work::Event(Event::Phase(Phase::Preparing)));
                let doc = Arc::new(self.backend.prepare(&self.bytes)?);
                let _ = tx.send(Work::Prepared(key, Arc::clone(&doc)));
                Ok(doc)
            }
        }
    }

    fn run(self, tx: &Sender<Work>, ctx: &egui::Context) -> Result<String, AiError> {
        let doc = self.document(tx)?;
        if self.cancel.load(Ordering::SeqCst) {
            return Err(AiError::Cancelled);
        }
        let mut sink = |e: Event| {
            let _ = tx.send(Work::Event(e));
            ctx.request_repaint();
        };
        self.backend.converse(&doc, &self.history, &self.request, &self.opts, &self.cancel, &mut sink)
    }
}

impl AiPane {
    pub(super) fn set_backend(&mut self, backend: Arc<dyn ChatBackend>) {
        self.backend = backend;
    }

    pub(super) fn running(&self) -> bool {
        self.run.is_some()
    }

    fn trim(&mut self) {
        let extra = (self.messages.len() + 2).saturating_sub(MAX_MESSAGES);
        if extra > 0 {
            self.messages.drain(..extra.min(self.messages.len()));
        }
    }

    /// The finished turns before the message being answered (errors and empty answers are not history).
    fn history(&self) -> Vec<Turn> {
        self.messages
            .iter()
            .filter(|m| !m.text.is_empty())
            .filter_map(|m| {
                let role = match m.who {
                    Who::User => Role::User,
                    Who::Assistant => Role::Assistant,
                    Who::Error => return None,
                };
                Some(Turn { role, text: m.text.clone(), model: m.model.clone() })
            })
            .collect()
    }

    /// Ask `request` about `source`: adds the question and an empty answer, and starts the worker.
    pub(super) fn start(&mut self, bytes: Arc<Vec<u8>>, request: Request, source: Source, opts: pdfcraft_ai::Options, ctx: &egui::Context) {
        if self.run.is_some() {
            return;
        }
        self.hint = None;
        self.trim();
        let history = self.history();
        let mut user = Msg::new(Who::User, prompt_text(&request));
        user.ask = Some((request.clone(), source.clone()));
        self.messages.push(user);
        self.messages.push(Msg::new(Who::Assistant, String::new()));
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let job = Job {
            backend: Arc::clone(&self.backend),
            bytes,
            source,
            cached: self.prepared.clone(),
            history,
            request,
            opts,
            cancel: Arc::clone(&cancel),
        };
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = job.run(&tx, &ctx);
            let _ = tx.send(Work::Done(result));
            ctx.request_repaint();
        });
        self.run = Some(Run { rx, cancel, phase: Phase::Thinking });
    }

    pub(super) fn stop(&mut self) {
        if let Some(run) = &self.run {
            run.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Remove the last answer (or error) and its question, and give back what was asked.
    pub(super) fn take_last_question(&mut self) -> Option<(Request, Source)> {
        if self.run.is_some() {
            return None;
        }
        while self.messages.last().is_some_and(|m| m.who != Who::User) {
            self.messages.pop();
        }
        self.messages.pop()?.ask
    }

    pub(super) fn new_chat(&mut self) {
        if self.run.is_none() {
            self.messages.clear();
            self.hint = None;
        }
    }

    /// Take what the worker has produced since the last frame.
    pub(super) fn poll(&mut self) {
        loop {
            let Some(run) = &self.run else { return };
            match run.rx.try_recv() {
                Ok(work) => self.apply(work),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    let e = AiError::AgentFailed(tl!("The assistant stopped unexpectedly.").to_string());
                    self.apply(Work::Done(Err(e)));
                }
            }
        }
    }

    fn apply(&mut self, work: Work) {
        match work {
            Work::Prepared(key, doc) => self.prepared = Some((key, doc)),
            Work::Event(Event::Phase(p)) => {
                if let Some(run) = &mut self.run {
                    run.phase = p;
                }
            }
            Work::Event(Event::Model(m)) => {
                if let Some(msg) = self.messages.last_mut() {
                    msg.model = Some(m);
                }
            }
            Work::Event(Event::Delta(d)) => {
                if let Some(msg) = self.messages.last_mut() {
                    msg.got_delta = true;
                    msg.force_parse |= d.contains('\n');
                    msg.text.push_str(&d);
                }
            }
            Work::Event(Event::Final(text)) => {
                if let Some(msg) = self.messages.last_mut().filter(|m| !m.got_delta) {
                    msg.text = text;
                }
            }
            Work::Done(result) => {
                self.run = None;
                self.finish(result);
            }
        }
    }

    fn finish(&mut self, result: Result<String, AiError>) {
        let Some(msg) = self.messages.last_mut() else { return };
        match result {
            Ok(answer) => {
                if !answer.is_empty() {
                    msg.text = answer;
                }
            }
            Err(AiError::Cancelled) => {
                msg.stopped = true;
                if msg.text.is_empty() {
                    self.messages.pop();
                }
            }
            Err(error) => {
                if msg.text.is_empty() {
                    msg.who = Who::Error;
                    msg.text = error.to_string();
                } else {
                    msg.stopped = true;
                    self.messages.push(Msg::new(Who::Error, error.to_string()));
                }
            }
        }
    }

    /// The conversation as Markdown, for Export.
    pub(super) fn transcript(&self) -> String {
        let mut out = String::new();
        for m in self.messages.iter().filter(|m| m.who != Who::Error && !m.text.is_empty()) {
            let head = match (m.who, &m.model) {
                (Who::User, _) => tl!("You").to_string(),
                (_, Some(model)) => format!("{} ({model})", tl!("Assistant")),
                _ => tl!("Assistant").to_string(),
            };
            out.push_str(&format!("## {head}\n\n{}\n\n", m.text));
        }
        out
    }
}

/// What a question is called in the chat.
fn prompt_text(request: &Request) -> String {
    match request {
        Request::Summary => tl!("Summarize this document").into(),
        Request::KeyPoints => tl!("Extract the key points").into(),
        Request::Explain => tl!("Explain it simply").into(),
        Request::ActionItems => tl!("Find actions, dates and risks").into(),
        Request::Translate { language } => crate::i18n::fmt(tl!("Translate the document into {language}"), &[("language", language)]),
        Request::Ask(question) => question.clone(),
        Request::TranslateText { text, .. } => text.clone(),
    }
}

/// What the header, composer and messages ask the frame to do once they are drawn.
#[derive(Default)]
pub(super) struct Actions {
    pub close: bool,
    pub send: Option<Request>,
    pub stop: bool,
    pub again: bool,
    pub new_chat: bool,
    pub export: bool,
    pub link: Option<String>,
    /// The pointer is over the panel (Esc stops an answer then).
    pub pointer_over: bool,
}

/// The source the context chip asks for, or why there is none.
fn source_for(app: &PdfCraftApp) -> Result<Source, String> {
    match app.ai.context {
        ContextChoice::Document => Ok(Source::Document),
        ContextChoice::Page => app.active_ids().and_then(|(i, _)| app.views.get(i)).map(|v| Source::Page(v.current)).ok_or_else(|| tl!("Open a document first.").to_string()),
        ContextChoice::Selection => app
            .active_ids()
            .and_then(|(i, _)| app.views.get(i))
            .and_then(|v| v.selected_text())
            .map(Source::Text)
            .ok_or_else(|| tl!("Select some text in the document first.").to_string()),
    }
}

pub fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    app.ai.poll();
    let t = Tokens::get(ui.ctx());
    let bytes = app.active_ids().and_then(|(_, id)| app.session.get(id)).map(|doc| doc.bytes.clone());
    let mut act = Actions::default();
    egui::Frame::NONE
        .fill(t.panel)
        .inner_margin(egui::Margin { left: 12, right: 10, top: 11, bottom: 9 })
        .stroke(Stroke::new(1.0, t.divider))
        .show(ui, |ui| {
            header::show(app, ui, &t, &mut act);
            ui.add_space(6.0);
            ui.separator();
            act.pointer_over = ui.rect_contains_pointer(ui.max_rect());
            egui::Panel::bottom("pdf-ai-composer").frame(egui::Frame::NONE).show_separator_line(false).show(ui, |ui| {
                ui.add_space(8.0);
                composer::show(app, ui, &t, bytes.is_some(), &mut act);
            });
            messages::show(app, ui, &t, &mut act);
        });
    if app.ai.running() {
        ui.ctx().request_repaint_after(REPARSE);
    }
    finish_frame(app, ui.ctx(), bytes, act);
}

fn finish_frame(app: &mut PdfCraftApp, ctx: &egui::Context, bytes: Option<Arc<Vec<u8>>>, act: Actions) {
    if act.close {
        app.choose_right_panel(None);
    }
    if let Some(url) = act.link {
        app.open_url(&url);
    }
    if act.stop {
        app.ai.stop();
    }
    if act.new_chat {
        app.ai.new_chat();
    }
    if act.export {
        export(app);
    }
    let mut ask = act.send.map(|r| source_for(app).map(|s| (r, s)));
    if act.again {
        ask = app.ai.take_last_question().map(Ok);
    }
    if ask.is_none()
        && let Some(request) = app.ai.queued.take()
    {
        ask = Some(Ok((request, Source::Document)));
    }
    match (ask, bytes) {
        (Some(Ok((request, source))), Some(bytes)) => send(app, ctx, bytes, request, source),
        (Some(Err(why)), _) => app.ai.hint = Some(why),
        _ => {}
    }
}

fn send(app: &mut PdfCraftApp, ctx: &egui::Context, bytes: Arc<Vec<u8>>, request: Request, source: Source) {
    // Page and selection text are extracted from the document: honour a no-copy permission.
    let extracted = !matches!(source, Source::Document);
    if extracted && let Some((_, id)) = app.active_ids() && !app.may_copy(id) {
        return;
    }
    if matches!(request, Request::Ask(_)) {
        app.ai.input.clear();
    }
    let opts = app.ai_choice.options();
    app.ai.start(bytes, request, source, opts, ctx);
}

fn export(app: &mut PdfCraftApp) {
    let text = app.ai.transcript();
    if text.is_empty() {
        return;
    }
    let dialog = rfd::AsyncFileDialog::new().set_file_name(format!("{}.md", tl!("AI conversation")));
    app.ask_one(crate::pickers::Ask::Save(dialog), None, move |app, path| match std::fs::write(&path, text) {
        Ok(()) => app.notify_tr("Conversation saved."),
        Err(e) => app.notify(format!("{}: {e}", tl!("Could not save the conversation"))),
    });
}

pub fn command(app: &mut PdfCraftApp, id: &str) {
    app.choose_right_panel(Some(RightPanel::Ai));
    app.ai.queued = match id {
        "ai.summary" => Some(Request::Summary),
        "ai.translate" => Some(Request::Translate { language: app.ai.translate_language.clone() }),
        _ => None,
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> Msg {
        Msg::new(Who::User, text.into())
    }

    #[test]
    fn history_leaves_out_errors_and_empty_answers() {
        let mut pane = AiPane::default();
        pane.messages.push(user("q1"));
        pane.messages.push(Msg::new(Who::Assistant, "a1".into()));
        pane.messages.push(Msg::new(Who::Error, "boom".into()));
        pane.messages.push(Msg::new(Who::Assistant, String::new()));
        let texts: Vec<_> = pane.history().into_iter().map(|t| t.text).collect();
        assert_eq!(texts, ["q1", "a1"]);
    }

    #[test]
    fn transcript_is_bounded_to_recent_messages() {
        let mut pane = AiPane::default();
        for n in 0..MAX_MESSAGES + 10 {
            pane.messages.push(user(&n.to_string()));
            pane.trim();
        }
        assert!(pane.messages.len() <= MAX_MESSAGES);
    }

    #[test]
    fn streaming_parse_waits_for_the_interval_but_not_for_a_line_break() {
        let mut m = Msg::new(Who::Assistant, String::new());
        m.text.push_str("one");
        let first = m.blocks(true);
        m.text.push_str(" two");
        let held = m.blocks(true);
        assert!(Arc::ptr_eq(&first, &held), "no re-parse inside the interval");
        m.force_parse = true;
        m.text.push('\n');
        assert!(!Arc::ptr_eq(&first, &m.blocks(true)), "a line break parses at once");
    }

    #[test]
    fn stop_without_text_removes_the_empty_answer_and_errors_become_messages() {
        let mut pane = AiPane::default();
        pane.messages.push(user("q"));
        pane.messages.push(Msg::new(Who::Assistant, String::new()));
        pane.finish(Err(AiError::Cancelled));
        assert_eq!(pane.messages.len(), 1);
        pane.messages.push(Msg::new(Who::Assistant, String::new()));
        pane.finish(Err(AiError::NoText));
        assert_eq!(pane.messages.last().map(|m| m.who), Some(Who::Error));
    }

    #[test]
    fn transcript_markdown_names_the_speaker_and_model() {
        let mut pane = AiPane::default();
        pane.messages.push(user("hello"));
        let mut a = Msg::new(Who::Assistant, "hi".into());
        a.model = Some("opus".into());
        pane.messages.push(a);
        let md = pane.transcript();
        assert!(md.contains("## You\n\nhello") && md.contains("## Assistant (opus)\n\nhi"));
    }
}
