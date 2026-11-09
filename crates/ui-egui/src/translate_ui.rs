//! The translation panel: translate pages of the open PDF into a new copy.
//!
//! The open document is never changed. The panel asks for the target language and the pages,
//! runs `pdfcraft_engine::translate::translate_copy` on a private copy in a worker thread, and
//! offers the result as a new tab or a saved file. The agent and model come from the choice the AI
//! panel shares (`PdfCraftApp::ai_choice`). Nothing is sent until the user presses Translate.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align2, Color32, CornerRadius, Sense, Stroke, vec2};
use pdfcraft_engine::translate::{AiTranslator, Progress, SkipReason, TranslateOptions, TranslationOutput, Translator, parse_page_range, translate_copy};
use pdfcraft_engine::{EditError, Session};

use crate::theme::{self, Tokens};
use crate::{PdfCraftApp, icons, widgets};

/// The languages offered as one-tap choices: the name sent to the agent (English, which every
/// agent understands) and the label shown.
const LANGUAGE_COUNT: usize = 10;

fn languages() -> [(&'static str, &'static str); LANGUAGE_COUNT] {
    [
        ("Spanish", tl!("Spanish")),
        ("English", tl!("English")),
        ("French", tl!("French")),
        ("German", tl!("German")),
        ("Portuguese", tl!("Portuguese")),
        ("Italian", tl!("Italian")),
        ("Chinese", tl!("Chinese")),
        ("Japanese", tl!("Japanese")),
        ("Arabic", tl!("Arabic")),
        ("Russian", tl!("Russian")),
    ]
}

/// Builds the translator for a run; tests replace it with an offline one.
pub type TranslatorFactory = Arc<dyn Fn(&pdfcraft_ai::Options) -> Box<dyn Translator> + Send + Sync>;

fn ai_factory() -> TranslatorFactory {
    Arc::new(|options| Box::new(AiTranslator { options: options.clone() }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    Page,
    All,
    Range,
}

enum Work {
    Progress(Progress),
    Done(Result<TranslationOutput, EditError>),
}

struct Run {
    rx: Receiver<Work>,
    cancel: Arc<AtomicBool>,
    done: usize,
    of: usize,
}

/// What a finished run reports, in the words the panel shows.
struct Summary {
    bytes: Arc<Vec<u8>>,
    name: String,
    pages: usize,
    translated: usize,
    no_text: usize,
    paragraphs_failed: usize,
    overflowed: usize,
    signed: bool,
}

enum Outcome {
    Done(Summary),
    Failed(String),
}

pub struct TranslatePane {
    /// Index into [`languages`]; one past the end means the typed name.
    language: usize,
    custom: String,
    scope: Scope,
    range: String,
    run: Option<Run>,
    outcome: Option<Outcome>,
    hint: Option<String>,
    /// The file name the running translation will give its copy.
    pending_name: Option<String>,
    factory: TranslatorFactory,
}

impl Default for TranslatePane {
    fn default() -> Self {
        Self { language: 0, custom: String::new(), scope: Scope::All, range: String::new(), run: None, outcome: None, hint: None, pending_name: None, factory: ai_factory() }
    }
}

impl TranslatePane {
    fn running(&self) -> bool {
        self.run.is_some()
    }

    fn language_name(&self) -> String {
        languages().get(self.language).map_or_else(|| self.custom.trim().to_owned(), |(name, _)| (*name).to_owned())
    }

    fn poll(&mut self) {
        loop {
            let Some(run) = &mut self.run else { return };
            match run.rx.try_recv() {
                Ok(Work::Progress(Progress::Started { pages })) => run.of = pages,
                Ok(Work::Progress(Progress::Page { done, of })) => {
                    run.done = done;
                    run.of = of;
                }
                Ok(Work::Done(result)) => {
                    self.run = None;
                    self.finish(result);
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.run = None;
                    self.outcome = Some(Outcome::Failed(tl!("The translation stopped unexpectedly.").to_string()));
                    return;
                }
            }
        }
    }

    fn finish(&mut self, result: Result<TranslationOutput, EditError>) {
        self.outcome = match result {
            Ok(out) => {
                let mut s = Summary {
                    bytes: Arc::new(out.bytes),
                    name: self.pending_name.take().unwrap_or_else(|| "translation.pdf".into()),
                    pages: out.report.pages.len(),
                    translated: 0,
                    no_text: 0,
                    paragraphs_failed: 0,
                    overflowed: 0,
                    signed: out.signed,
                };
                for p in &out.report.pages {
                    match (&p.skipped, &p.error) {
                        (Some(SkipReason::NoText), _) => s.no_text += 1,
                        (None, None) => s.translated += 1,
                        _ => {}
                    }
                    s.paragraphs_failed += p.failed + p.lost;
                    s.overflowed += p.overflowed;
                }
                Some(Outcome::Done(s))
            }
            Err(EditError::Cancelled) => None,
            Err(e) => Some(Outcome::Failed(e.to_string())),
        };
    }
}

/// What the panel asks the frame to do once it is drawn.
#[derive(Default)]
struct Actions {
    close: bool,
    start: bool,
    cancel: bool,
    open: bool,
    save: bool,
}

pub fn panel(app: &mut PdfCraftApp, ui: &mut egui::Ui) {
    app.translate.poll();
    let t = Tokens::get(ui.ctx());
    let mut act = Actions::default();
    let has_doc = app.active_ids().is_some();
    egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin { left: 12, right: 10, top: 11, bottom: 9 }).stroke(Stroke::new(1.0, t.divider)).show(ui, |ui| {
        header(app, ui, &t, &mut act);
        ui.add_space(6.0);
        ui.separator();
        egui::Panel::bottom("pdf-translate-action").frame(egui::Frame::NONE).show_separator_line(false).show(ui, |ui| {
            ui.add_space(8.0);
            action_area(app, ui, &t, has_doc, &mut act);
        });
        egui::ScrollArea::vertical().id_salt("pdf-translate-body").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add_space(10.0);
            let editable = !app.translate.running();
            ui.add_enabled_ui(editable, |ui| {
                language_section(app, ui, &t);
                ui.add_space(16.0);
                pages_section(app, ui, &t);
            });
            ui.add_space(14.0);
            privacy(ui, &t);
            if let Some(outcome) = &app.translate.outcome {
                ui.add_space(14.0);
                outcome_card(outcome, ui, &t, &mut act);
            }
        });
    });
    if app.translate.running() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    finish_frame(app, ui.ctx(), act);
}

fn header(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, act: &mut Actions) {
    ui.horizontal(|ui| {
        egui::Frame::NONE.fill(t.accent_soft).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.add(icons::image("languages", 17.0, t.accent_text));
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(egui::RichText::new(tl!("Translate")).font(theme::semibold(15.5)).color(t.text));
            ui.label(egui::RichText::new(tl!("Into a new copy of this PDF")).font(theme::regular(11.5)).color(t.text_muted));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icons::button(ui, "x", 26.0, false, tl!("Close")).clicked() {
                act.close = true;
            }
        });
    });
    ui.add_space(8.0);
    egui::Frame::NONE.fill(t.hover).stroke(Stroke::new(1.0, t.divider)).corner_radius(CornerRadius::same(10)).inner_margin(egui::Margin::symmetric(9, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add_enabled_ui(!app.translate.running(), |ui| crate::ai_model::picker(ui, &mut app.ai_choice, t));
    });
}

fn section_title(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.label(egui::RichText::new(text).font(theme::semibold(12.0)).color(t.text_muted));
    ui.add_space(6.0);
}

fn language_section(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    section_title(ui, t, tl!("Translate into"));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        for (i, (_, label)) in languages().iter().enumerate() {
            if choice_pill(ui, t, label, app.translate.language == i).clicked() {
                app.translate.language = i;
            }
        }
        if choice_pill(ui, t, tl!("Other…"), app.translate.language == LANGUAGE_COUNT).clicked() {
            app.translate.language = LANGUAGE_COUNT;
        }
    });
    if app.translate.language == LANGUAGE_COUNT {
        ui.add_space(8.0);
        ui.add(widgets::line(&mut app.translate.custom).desired_width(f32::INFINITY).hint_text(tl!("Language name, e.g. Catalan")));
    }
}

fn pages_section(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens) {
    section_title(ui, t, tl!("Pages"));
    segmented(ui, t, &mut app.translate.scope, &[(Scope::Page, tl!("Current page")), (Scope::All, tl!("All pages")), (Scope::Range, tl!("Range"))]);
    if app.translate.scope == Scope::Range {
        ui.add_space(8.0);
        ui.add(widgets::line(&mut app.translate.range).desired_width(f32::INFINITY).hint_text(tl!("e.g. 1-3, 5")));
    }
}

fn privacy(ui: &mut egui::Ui, t: &Tokens) {
    ui.horizontal_top(|ui| {
        ui.add(icons::image("shield-check", 13.0, t.text_faint));
        ui.add(egui::Label::new(egui::RichText::new(tl!("The original is never changed. Page text is sent to the chosen agent only when you press Translate.")).font(theme::regular(11.0)).color(t.text_faint)).wrap());
    });
}

/// A one-tap choice pill; the chosen one is tinted.
fn choice_pill(ui: &mut egui::Ui, t: &Tokens, label: &str, selected: bool) -> egui::Response {
    let ink = if selected { t.accent_text } else { t.text };
    let galley = crate::bidi::layout_ui(ui, label.to_owned(), theme::medium(12.5), ink);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 26.0, 28.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), selected, label));
    let press = theme::Press::track(ui, &resp);
    let (fill, stroke) = if selected { (t.accent_soft, Stroke::new(1.0, t.accent)) } else { (press.fill(t.card, t.hover, t.pressed), Stroke::new(1.0, t.border)) };
    ui.painter().rect(rect, CornerRadius::same(14), fill, stroke, egui::StrokeKind::Inside);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    theme::hand(resp)
}

/// A row of equal segments in one rounded track; the chosen one is raised.
fn segmented<T: Copy + PartialEq>(ui: &mut egui::Ui, t: &Tokens, value: &mut T, items: &[(T, &str)]) {
    let width = ui.available_width();
    let (track, _) = ui.allocate_exact_size(vec2(width, 34.0), Sense::hover());
    ui.painter().rect_filled(track, CornerRadius::same(10), t.hover);
    ui.painter().rect_stroke(track, CornerRadius::same(10), Stroke::new(1.0, t.divider), egui::StrokeKind::Inside);
    let seg_w = (track.width() - 6.0) / items.len().max(1) as f32;
    for (i, (v, label)) in items.iter().enumerate() {
        let rect = egui::Rect::from_min_size(track.min + vec2(3.0 + seg_w * i as f32, 3.0), vec2(seg_w, 28.0));
        let resp = ui.interact(rect, ui.scope_id().with(("segment", i)), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, ui.is_enabled(), *value == *v, *label));
        let on = *value == *v;
        if on {
            ui.painter().rect(rect, CornerRadius::same(8), t.card, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(8), t.pressed.gamma_multiply(0.5));
        }
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, *label, theme::medium(12.5), if on { t.text } else { t.text_muted });
        if resp.clicked() {
            *value = *v;
        }
        theme::hand(resp);
    }
}

/// The full-width button at the bottom: Translate, or the progress and Cancel while it runs.
fn action_area(app: &mut PdfCraftApp, ui: &mut egui::Ui, t: &Tokens, has_doc: bool, act: &mut Actions) {
    if let Some(run) = &app.translate.run {
        let frac = if run.of == 0 { 0.0 } else { run.done as f32 / run.of as f32 };
        let text = if run.of == 0 {
            tl!("Preparing…").to_string()
        } else {
            crate::i18n::fmt(tl!("Translating page {n} of {total}…"), &[("n", &run.done.saturating_add(1).min(run.of).to_string()), ("total", &run.of.to_string())])
        };
        ui.label(egui::RichText::new(text).font(theme::medium(12.5)).color(t.text_muted));
        ui.add_space(6.0);
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
        ui.painter().rect_filled(bar, CornerRadius::same(3), t.pressed);
        let mut fill = bar;
        fill.set_width((bar.width() * frac).max(if run.of == 0 { 0.0 } else { 6.0 }));
        ui.painter().rect_filled(fill, CornerRadius::same(3), t.accent);
        ui.add_space(10.0);
        if widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
            act.cancel = true;
        }
    } else {
        if let Some(hint) = &app.translate.hint {
            ui.label(egui::RichText::new(hint).small().color(ui.visuals().error_fg_color));
            ui.add_space(6.0);
        }
        let again = app.translate.outcome.is_some();
        let label = if again { tl!("Translate again") } else { tl!("Start translation") };
        if primary_button(ui, t, label, has_doc).clicked() && has_doc {
            act.start = true;
        }
    }
    ui.add_space(2.0);
}

fn primary_button(ui: &mut egui::Ui, t: &Tokens, label: &str, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), if enabled { Sense::click() } else { Sense::hover() });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let press = theme::Press::track(ui, &resp);
    let fill = if enabled { press.fill(t.accent, t.accent_text, t.accent.lerp_to_gamma(Color32::BLACK, 0.22)) } else { t.pressed };
    let ink = if enabled { Color32::WHITE } else { t.text_faint };
    ui.painter().rect_filled(rect, CornerRadius::same(12), fill);
    let galley = crate::bidi::layout_ui(ui, label.to_owned(), theme::semibold(13.5), ink);
    let icon = 16.0;
    let total = icon + 8.0 + galley.size().x;
    let left = rect.center().x - total / 2.0;
    icons::paint(ui, egui::Rect::from_min_size(egui::pos2(left, rect.center().y - icon / 2.0), vec2(icon, icon)), "languages", icon, ink);
    ui.painter().galley(egui::pos2(left + icon + 8.0, rect.center().y - galley.size().y / 2.0), galley, ink);
    if enabled { theme::hand(resp) } else { resp }
}

fn outcome_card(outcome: &Outcome, ui: &mut egui::Ui, t: &Tokens, act: &mut Actions) {
    match outcome {
        Outcome::Failed(why) => {
            let red = ui.visuals().error_fg_color;
            egui::Frame::NONE.fill(red.gamma_multiply(0.09)).stroke(Stroke::new(1.0, red.gamma_multiply(0.55))).corner_radius(CornerRadius::same(11)).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_top(|ui| {
                    ui.add(icons::image("triangle-alert", 15.0, red));
                    ui.add(egui::Label::new(egui::RichText::new(why).color(t.text)).wrap());
                });
            });
        }
        Outcome::Done(s) => {
            egui::Frame::NONE.fill(t.card).stroke(Stroke::new(1.0, t.border)).corner_radius(CornerRadius::same(12)).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(icons::image("check-circle-2", 18.0, t.accent_text));
                    ui.label(egui::RichText::new(tl!("Translation ready")).font(theme::semibold(14.0)).color(t.text));
                });
                ui.add_space(4.0);
                let n = s.translated.to_string();
                let total = s.pages.to_string();
                ui.label(egui::RichText::new(crate::i18n::fmt(tl!("{n} of {total} pages translated"), &[("n", &n), ("total", &total)])).font(theme::regular(12.5)).color(t.text_muted));
                let mut notes = Vec::new();
                if s.no_text > 0 {
                    notes.push(crate::i18n::fmt(tl!("{n} pages without text were left as they are"), &[("n", &s.no_text.to_string())]));
                }
                if s.paragraphs_failed > 0 {
                    notes.push(crate::i18n::fmt(tl!("{n} paragraphs could not be translated"), &[("n", &s.paragraphs_failed.to_string())]));
                }
                if s.overflowed > 0 {
                    notes.push(crate::i18n::fmt(tl!("{n} paragraphs are tight for their box"), &[("n", &s.overflowed.to_string())]));
                }
                for note in notes {
                    ui.label(egui::RichText::new(note).font(theme::regular(11.5)).color(t.text_faint));
                }
                if s.signed {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(tl!("The copy does not keep the original's signatures.")).font(theme::regular(11.5)).color(ui.visuals().warn_fg_color));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if widgets::pill_button(ui, tl!("Open copy"), true).clicked() {
                        act.open = true;
                    }
                    if widgets::pill_button(ui, tl!("Save as…"), false).clicked() {
                        act.save = true;
                    }
                });
            });
        }
    }
}

fn finish_frame(app: &mut PdfCraftApp, ctx: &egui::Context, act: Actions) {
    if act.close {
        app.choose_right_panel(None);
    }
    if act.cancel
        && let Some(run) = &app.translate.run
    {
        run.cancel.store(true, Ordering::SeqCst);
    }
    if act.open
        && let Some(Outcome::Done(s)) = &app.translate.outcome
    {
        let (name, bytes) = (s.name.clone(), s.bytes.to_vec());
        if let Err(e) = app.open_bytes(&name, None, bytes) {
            app.translate.outcome = Some(Outcome::Failed(e));
        }
    }
    if act.save
        && let Some(Outcome::Done(s)) = &app.translate.outcome
    {
        let (name, bytes) = (s.name.clone(), s.bytes.clone());
        let dialog = rfd::AsyncFileDialog::new().set_file_name(name).add_filter("PDF", &["pdf"]);
        app.ask_one(crate::pickers::Ask::Save(dialog), None, move |app, path| match std::fs::write(&path, bytes.as_slice()) {
            Ok(()) => app.notify_tr("Translated copy saved."),
            Err(e) => app.notify(format!("{}: {e}", tl!("Could not save the copy"))),
        });
    }
    if act.start {
        start(app, ctx);
    }
}

/// The file name of the copy: `Report.pdf` becomes `Report (Spanish).pdf`.
fn copy_name(doc_name: &str, language: &str) -> String {
    let stem = doc_name.strip_suffix(".pdf").or_else(|| doc_name.strip_suffix(".PDF")).unwrap_or(doc_name);
    format!("{stem} ({language}).pdf")
}

fn start(app: &mut PdfCraftApp, ctx: &egui::Context) {
    app.translate.hint = None;
    let language = app.translate.language_name();
    if language.is_empty() {
        app.translate.hint = Some(tl!("Type the language to translate into.").to_string());
        return;
    }
    let Some((index, id)) = app.active_ids() else { return };
    if !app.may_copy(id) {
        app.translate.hint = Some(tl!("This document does not allow copying its text.").to_string());
        return;
    }
    let count = app.session.get(id).map_or(0, |d| d.info.pages.len());
    let pages = match app.translate.scope {
        Scope::All => (0..count).collect::<Vec<_>>(),
        Scope::Page => vec![app.views.get(index).map_or(0, |v| v.current)],
        Scope::Range => match parse_page_range(&app.translate.range, count) {
            Ok(p) => p,
            Err(_) => {
                app.translate.hint = Some(tl!("That page range is not valid.").to_string());
                return;
            }
        },
    };
    // The bytes the document would save now, so unsaved edits are translated too.
    let Ok(bytes) = app.session.save_bytes(id) else {
        app.translate.hint = Some(tl!("The document could not be read.").to_string());
        return;
    };
    let (name, password) = app.session.get(id).map(|d| (d.name.clone(), d.password.clone())).unwrap_or_default();
    let options = app.ai_choice.options();
    let factory = Arc::clone(&app.translate.factory);
    let copy = copy_name(&name, &language);
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    let worker_cancel = Arc::clone(&cancel);
    std::thread::spawn(move || {
        let result = (|| {
            let mut session = Session::new();
            let doc = session.open(name, None, bytes, password.as_deref()).map_err(|e| EditError::Reopen(e.to_string()))?;
            let translator = factory(&options);
            let mut progress = |p: Progress| {
                let _ = tx.send(Work::Progress(p));
                ctx.request_repaint();
            };
            translate_copy(&session, doc, &pages, &language, &TranslateOptions::default(), translator.as_ref(), &mut progress, &worker_cancel)
        })();
        let _ = tx.send(Work::Done(result));
        ctx.request_repaint();
    });
    app.translate.outcome = None;
    app.translate.pending_name = Some(copy);
    app.translate.run = Some(Run { rx, cancel, done: 0, of: 0 });
}

impl PdfCraftApp {
    /// Replace how the translation panel builds its translator (tests use an offline one).
    pub fn set_translator_factory(&mut self, factory: TranslatorFactory) {
        self.translate.factory = factory;
    }
}
