//! Paints parsed Markdown blocks with egui, in the app's own tokens (light and dark). Links are
//! never opened here: the url the user clicked is returned to the caller, which owns opening.

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Color32, FontFamily, FontId, Frame, Id, Label, Layout, Margin, RichText, ScrollArea, Stroke, TextWrapMode, Ui, vec2};

use super::highlight::{self, Tok};
use super::parse::{Align as ColAlign, Block, Inline, plain};
use crate::icons;
use crate::theme::{self, Tokens};

/// Render recursion guard; the parser already caps nesting at 32, this only protects hand-built trees.
const MAX_RENDER_DEPTH: usize = 40;
/// Columns drawn per table; the rest are dropped.
const MAX_COLUMNS: usize = 64;

/// Paints `blocks` into `ui` and returns the url of a link the user clicked this frame, if it is
/// an http, https or mailto url. `id_salt` must differ between messages shown at once.
pub fn show(ui: &mut Ui, blocks: &[Block], theme: &Tokens, id_salt: Id) -> Option<String> {
    let mut cx = Cx { t: theme, clicked: None };
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 8.0;
        blocks_ui(ui, &mut cx, blocks, id_salt, 0);
    });
    cx.clicked
}

struct Cx<'a> {
    t: &'a Tokens,
    clicked: Option<String>,
}

/// Only these schemes are ever offered to the caller.
fn safe_url(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    !u.chars().any(char::is_control) && (u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:"))
}

#[derive(Clone, Copy)]
struct Style {
    size: f32,
    color: Color32,
    strong: bool,
    emph: bool,
    strike: bool,
}

impl Style {
    fn font(&self) -> FontId {
        if self.strong { theme::semibold(self.size) } else { FontId::new(self.size, FontFamily::Proportional) }
    }

    fn format(&self) -> TextFormat {
        TextFormat {
            font_id: self.font(),
            color: self.color,
            italics: self.emph,
            strikethrough: if self.strike { Stroke::new(1.0, self.color) } else { Stroke::NONE },
            ..Default::default()
        }
    }
}

/// A run of text, or a clickable link, in paint order.
enum Seg {
    Text(LayoutJob),
    Link { text: String, url: String, style: Style },
}

struct Writer<'a> {
    t: &'a Tokens,
    segs: Vec<Seg>,
    cur: LayoutJob,
}

impl Writer<'_> {
    fn flush(&mut self) {
        if !self.cur.text.is_empty() {
            self.segs.push(Seg::Text(std::mem::take(&mut self.cur)));
        }
    }

    fn add(&mut self, inl: &[Inline], s: Style) {
        for i in inl {
            match i {
                Inline::Text(x) => self.cur.append(x, 0.0, s.format()),
                Inline::Strong(v) => self.add(v, Style { strong: true, ..s }),
                Inline::Emphasis(v) => self.add(v, Style { emph: true, ..s }),
                Inline::Strike(v) => self.add(v, Style { strike: true, ..s }),
                Inline::Code(x) => {
                    let f = TextFormat {
                        font_id: FontId::monospace(s.size * 0.92),
                        background: self.t.hover,
                        ..s.format()
                    };
                    self.cur.append(x, 0.0, f);
                }
                Inline::Math(x) => {
                    let f = TextFormat { font_id: FontId::monospace(s.size * 0.92), italics: true, ..s.format() };
                    self.cur.append(&math_text(x), 0.0, f);
                }
                Inline::Break => self.cur.append("\n", 0.0, s.format()),
                Inline::Link { text, url } => {
                    if safe_url(url) {
                        self.flush();
                        self.segs.push(Seg::Link { text: plain(text), url: url.clone(), style: s });
                    } else {
                        self.add(text, s);
                    }
                }
            }
        }
    }
}

fn inline_ui(ui: &mut Ui, cx: &mut Cx<'_>, inl: &[Inline], style: Style, wrap: bool) {
    let mut w = Writer { t: cx.t, segs: Vec::new(), cur: LayoutJob::default() };
    w.add(inl, style);
    w.flush();
    let mode = if wrap { TextWrapMode::Wrap } else { TextWrapMode::Extend };
    let mut segs = w.segs;
    if segs.len() <= 1 && !matches!(segs.first(), Some(Seg::Link { .. })) {
        if let Some(Seg::Text(job)) = segs.pop() {
            ui.add(Label::new(job).wrap_mode(mode));
        }
        return;
    }
    let body = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for seg in segs {
            match seg {
                Seg::Text(job) => {
                    ui.add(Label::new(job).wrap_mode(mode));
                }
                Seg::Link { text, url, style } => {
                    let rt = RichText::new(text).font(style.font()).color(cx.t.accent_text);
                    let r = ui.add(egui::Link::new(rt)).on_hover_text(url.as_str());
                    if r.clicked() {
                        cx.clicked = Some(url);
                    }
                }
            }
        }
    };
    if wrap {
        ui.horizontal_wrapped(body);
    } else {
        ui.horizontal(body);
    }
}

fn base_style(ui: &Ui, t: &Tokens) -> Style {
    Style { size: egui::TextStyle::Body.resolve(ui.style()).size, color: t.text, strong: false, emph: false, strike: false }
}

fn blocks_ui(ui: &mut Ui, cx: &mut Cx<'_>, blocks: &[Block], id: Id, depth: usize) {
    if depth > MAX_RENDER_DEPTH {
        return;
    }
    for (i, b) in blocks.iter().enumerate() {
        block_ui(ui, cx, b, id.with(i), depth);
    }
}

fn block_ui(ui: &mut Ui, cx: &mut Cx<'_>, b: &Block, id: Id, depth: usize) {
    let base = base_style(ui, cx.t);
    match b {
        Block::Paragraph(inl) => inline_ui(ui, cx, inl, base, true),
        Block::Heading { level, inlines } => {
            let scale = match level {
                1 => 1.5,
                2 => 1.3,
                3 => 1.15,
                _ => 1.05,
            };
            inline_ui(ui, cx, inlines, Style { size: base.size * scale, strong: true, ..base }, true);
        }
        Block::List { ordered, start, items, checked } => list_ui(ui, cx, (*ordered, *start), items, checked, id, depth),
        Block::Quote(inner) => quote_ui(ui, cx, inner, id, depth),
        Block::Code { lang, text } => code_ui(ui, cx, lang.as_deref(), text, id),
        Block::Table { align, head, rows } => table_ui(ui, cx, align, head, rows, id),
        Block::Rule => {
            ui.separator();
        }
    }
}

fn list_ui(ui: &mut Ui, cx: &mut Cx<'_>, kind: (bool, u64), items: &[Vec<Block>], checked: &[Option<bool>], id: Id, depth: usize) {
    let (ordered, start) = kind;
    let t = cx.t;
    ui.spacing_mut().item_spacing.y = 4.0;
    for (i, item) in items.iter().enumerate() {
        let marker = match checked.get(i).copied().flatten() {
            Some(true) => "[x]".to_owned(),
            Some(false) => "[ ]".to_owned(),
            None if ordered => format!("{}.", start.saturating_add(i as u64)),
            None if depth == 0 => "\u{2022}".to_owned(),
            None => "\u{2013}".to_owned(),
        };
        let item_id = id.with(i);
        ui.horizontal_top(|ui| {
            ui.scope(|ui| {
                ui.set_min_width(if ordered { 26.0 } else { 18.0 });
                let rt = RichText::new(marker).color(t.text_muted);
                ui.label(if checked.get(i).copied().flatten().is_some() { rt.monospace() } else { rt });
            });
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                blocks_ui(ui, cx, item, item_id, depth + 1);
            });
        });
    }
}

fn quote_ui(ui: &mut Ui, cx: &mut Cx<'_>, inner: &[Block], id: Id, depth: usize) {
    let bar = cx.t.border;
    let r = Frame::new().inner_margin(Margin { left: 12, right: 0, top: 2, bottom: 2 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        blocks_ui(ui, cx, inner, id, depth + 1);
    });
    let rect = r.response.rect;
    ui.painter().line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(3.0, bar));
}

fn tok_format(tok: Tok, t: &Tokens, size: f32) -> TextFormat {
    let mono = FontId::monospace(size);
    let string = t.text.lerp_to_gamma(t.accent, 0.6);
    match tok {
        Tok::Plain => TextFormat { font_id: mono, color: t.text, ..Default::default() },
        Tok::Keyword => TextFormat { font_id: mono, color: t.accent_text, ..Default::default() },
        Tok::Str => TextFormat { font_id: mono, color: string, ..Default::default() },
        Tok::Comment => TextFormat { font_id: mono, color: t.text_faint, italics: true, ..Default::default() },
        Tok::Number => TextFormat { font_id: mono, color: t.accent, ..Default::default() },
    }
}

fn code_ui(ui: &mut Ui, cx: &mut Cx<'_>, lang: Option<&str>, text: &str, id: Id) {
    let t = cx.t;
    let size = egui::TextStyle::Body.resolve(ui.style()).size * 0.92;
    let code = text.strip_suffix('\n').unwrap_or(text);
    Frame::new()
        .fill(t.pasteboard)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(t.radius)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(lang.unwrap_or("")).small().color(t.text_faint));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icons::button(ui, "copy", 24.0, false, tl!("Copy")).clicked() {
                        ui.ctx().copy_text(code.to_owned());
                    }
                });
            });
            let mut job = LayoutJob::default();
            for (tok, run) in highlight::tokens(lang, code) {
                job.append(run, 0.0, tok_format(tok, t, size));
            }
            ScrollArea::horizontal().id_salt(id.with("code")).auto_shrink([false, true]).show(ui, |ui| {
                ui.add(Label::new(job).wrap_mode(TextWrapMode::Extend));
            });
        });
}

fn table_ui(ui: &mut Ui, cx: &mut Cx<'_>, align: &[ColAlign], head: &[Vec<Inline>], rows: &[Vec<Vec<Inline>>], id: Id) {
    let cols = rows.iter().map(Vec::len).chain([head.len(), align.len()]).max().unwrap_or(0).min(MAX_COLUMNS);
    if cols == 0 {
        return;
    }
    let base = base_style(ui, cx.t);
    let t = cx.t;
    ScrollArea::horizontal().id_salt(id.with("table")).auto_shrink([false, true]).show(ui, |ui| {
        egui::Grid::new(id.with("grid")).num_columns(cols).striped(true).spacing(vec2(16.0, 6.0)).show(ui, |ui| {
            let mut row = |ui: &mut Ui, cells: &[Vec<Inline>], style: Style| {
                for c in 0..cols {
                    let a = match align.get(c) {
                        Some(ColAlign::Center) => Align::Center,
                        Some(ColAlign::Right) => Align::Max,
                        _ => Align::Min,
                    };
                    let empty = Vec::new();
                    let cell = cells.get(c).unwrap_or(&empty);
                    ui.with_layout(Layout::top_down(a), |ui| inline_ui(ui, cx, cell, style, false));
                }
                ui.end_row();
            };
            row(ui, head, Style { strong: true, color: t.text, ..base });
            for r in rows {
                row(ui, r, base);
            }
        });
    });
}

/// Replaces a few common LaTeX commands and `^`/`_` digits with Unicode. This is NOT LaTeX: no
/// fractions, roots, matrices or layout; anything it does not know is shown as written.
pub fn math_text(src: &str) -> String {
    const WORDS: &[(&str, &str)] = &[
        ("alpha", "α"), ("beta", "β"), ("gamma", "γ"), ("delta", "δ"), ("epsilon", "ε"), ("theta", "θ"), ("lambda", "λ"),
        ("mu", "μ"), ("pi", "π"), ("rho", "ρ"), ("sigma", "σ"), ("tau", "τ"), ("phi", "φ"), ("omega", "ω"), ("Delta", "Δ"),
        ("Sigma", "Σ"), ("Omega", "Ω"), ("times", "×"), ("cdot", "·"), ("pm", "±"), ("leq", "≤"), ("le", "≤"), ("geq", "≥"),
        ("ge", "≥"), ("neq", "≠"), ("ne", "≠"), ("approx", "≈"), ("rightarrow", "→"), ("to", "→"), ("leftarrow", "←"),
        ("infty", "∞"), ("sum", "∑"), ("int", "∫"), ("sqrt", "√"), ("in", "∈"), ("forall", "∀"), ("exists", "∃"),
    ];
    const SUP: &str = "⁰¹²³⁴⁵⁶⁷⁸⁹";
    const SUB: &str = "₀₁₂₃₄₅₆₇₈₉";
    let mut out = String::with_capacity(src.len());
    let mut chars = src.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                let rest = src.get(i.saturating_add(1)..).unwrap_or("");
                let n = rest.find(|ch: char| !ch.is_ascii_alphabetic()).unwrap_or(rest.len());
                let word = rest.get(..n).unwrap_or("");
                match WORDS.iter().find(|(w, _)| *w == word) {
                    Some((_, sym)) if n > 0 => {
                        out.push_str(sym);
                        for _ in 0..n {
                            chars.next();
                        }
                    }
                    _ => out.push('\\'),
                }
            }
            '^' | '_' => {
                let table = if c == '^' { SUP } else { SUB };
                match chars.peek().and_then(|&(_, d)| d.to_digit(10)) {
                    Some(d) => {
                        out.extend(table.chars().nth(d as usize));
                        chars.next();
                    }
                    None => out.push(c),
                }
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_substitutions() {
        assert_eq!(math_text(r"\alpha + \beta^2 \leq \pi \times x_1"), "α + β² ≤ π × x₁");
        assert_eq!(math_text(r"a \rightarrow \infty"), "a → ∞");
        assert_eq!(math_text(r"\unknown ñ\"), r"\unknown ñ\");
        assert_eq!(math_text("x^n"), "x^n");
    }

    #[test]
    fn only_web_and_mail_links() {
        assert!(safe_url("https://a.org"));
        assert!(safe_url("HTTP://a.org"));
        assert!(safe_url("mailto:a@b.c"));
        assert!(!safe_url("javascript:alert(1)"));
        assert!(!safe_url("file:///etc/passwd"));
        assert!(!safe_url("https://a\u{0}.org"));
    }
}
