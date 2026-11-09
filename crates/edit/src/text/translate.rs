//! Translate the text of pages in place, keeping their layout.
//!
//! The orchestration is independent of any model or UI: a [`Translator`] turns segments into
//! segments, and [`translate_document`] reads each page's paragraphs (`text_blocks`), marks the
//! runs of mixed-style paragraphs (`tag_runs`), sends the segments in batches, restores the
//! styles (`untag_runs`) and rewrites the page (`rewrite_page_blocks`).

use std::sync::atomic::{AtomicBool, Ordering};

use pdfcraft_cos::Document;

use super::{BlockEdit, BlockKey, BlockOutcome, TaggedBlock, TextBlock, is_translatable, rewrite_page_blocks, tag_runs, text_blocks, untag_runs};

/// Turns segments of text into the same number of segments in `language`.
///
/// Marks such as `⟦1⟧…⟦/1⟧` and `⟦br⟧` must be kept around the equivalent words. An `Err` (or a
/// different number of segments) makes the caller retry the batch. Long calls should watch `cancel`.
pub trait Translator {
    fn translate(&self, language: &str, segments: &[String], cancel: &AtomicBool) -> Result<Vec<String>, String>;
}

/// Knobs of [`translate_document`].
#[derive(Clone, Debug, PartialEq)]
pub struct TranslateOptions {
    /// Smallest font scale a paragraph may shrink to when the translation is longer (0.7..=1.0).
    pub min_scale: f64,
    /// Most characters sent to the translator per call (a bigger segment goes alone).
    pub batch_chars: usize,
    /// Extra attempts for a batch that failed or came back with the wrong number of segments.
    pub retries: usize,
}

impl Default for TranslateOptions {
    fn default() -> Self {
        Self { min_scale: 0.7, batch_chars: 12_000, retries: 1 }
    }
}

/// Where [`translate_document`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    /// Before the first page; `pages` is the number of pages asked for.
    Started { pages: usize },
    /// A page was dealt with (translated, skipped or failed).
    Page { done: usize, of: usize },
}

/// Why a page was not translated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// No translatable text (a scanned page, or only numbers and symbols): nothing was sent.
    NoText,
    /// The run was cancelled before the page was rewritten.
    Cancelled,
}

/// What happened to one requested page. The counts are paragraphs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageReport {
    /// 0-based page index, as asked for.
    pub page: usize,
    pub applied: usize,
    pub unchanged: usize,
    pub lost: usize,
    pub merged: usize,
    pub failed: usize,
    /// Applied, but still too long for the box at the smallest size.
    pub overflowed: usize,
    /// Applied with a substitute font.
    pub substituted: usize,
    /// Applied with the paragraph's own style because the marked styles couldn't be used.
    pub plain: usize,
    pub skipped: Option<SkipReason>,
    pub error: Option<String>,
}

/// One entry per page asked for, in the order given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranslationReport {
    pub pages: Vec<PageReport>,
    pub cancelled: bool,
}

/// Rows of code or data (assignments, brackets, quotes) must keep their line breaks.
pub fn looks_like_code(text: &str) -> bool {
    text.chars().filter(|c| matches!(c, '=' | '[' | ']' | '{' | '}' | '(' | ')' | '"' | ';')).count() >= 4
}

/// Translates `pages` (0-based) of `doc` in place. `doc` is the caller's working copy.
///
/// Out-of-range and repeated pages are ignored and say so in their [`PageReport::error`]. When
/// `cancel` is set, the pages already rewritten stay and the rest are reported as cancelled.
pub fn translate_document(
    doc: &mut Document,
    pages: &[usize],
    language: &str,
    opts: &TranslateOptions,
    tr: &dyn Translator,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> TranslationReport {
    let total = pdfcraft_model::pages(doc).len();
    let of = pages.len();
    let mut report = TranslationReport::default();
    progress(Progress::Started { pages: of });
    let mut seen: Vec<usize> = Vec::new();
    for (index, &page) in pages.iter().enumerate() {
        let mut entry = PageReport { page, ..PageReport::default() };
        if report.cancelled || cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            entry.skipped = Some(SkipReason::Cancelled);
        } else if page >= total {
            entry.error = Some(format!("page {} is out of range ({total} pages)", page.saturating_add(1)));
        } else if seen.contains(&page) {
            entry.error = Some(format!("page {} is repeated and was ignored", page.saturating_add(1)));
        } else {
            seen.push(page);
            if let Err(e) = translate_page(doc, page, language, opts, tr, cancel, &mut entry) {
                entry.error = Some(e);
            }
            if entry.skipped == Some(SkipReason::Cancelled) {
                report.cancelled = true;
            }
        }
        report.pages.push(entry);
        progress(Progress::Page { done: index.saturating_add(1), of });
    }
    report
}

/// A paragraph ready to send: its text, and its marked runs when it had mixed styles.
struct Prepared {
    block: TextBlock,
    marked: Option<TaggedBlock>,
    source: String,
}

fn translate_page(
    doc: &mut Document,
    page: usize,
    language: &str,
    opts: &TranslateOptions,
    tr: &dyn Translator,
    cancel: &AtomicBool,
    entry: &mut PageReport,
) -> Result<(), String> {
    let blocks = text_blocks(doc, page).map_err(|e| format!("could not read the page text: {e}"))?;
    // Prose goes as one flowing line (its breaks are only where the old width wrapped it); code
    // keeps its rows; mixed styles and hard breaks go marked.
    let mut prepared = Vec::with_capacity(blocks.len());
    for block in blocks {
        let code = looks_like_code(&block.text);
        let marked = if code { None } else { tag_runs(doc, page, &block).map_err(|e| format!("could not read the styles: {e}"))? };
        let source = match &marked {
            Some(m) => m.text.clone(),
            None if code => block.reflow_text.clone(),
            None => block.text.clone(),
        };
        prepared.push(Prepared { block, marked, source });
    }
    // Paragraphs without letters never go out and stay as they are.
    let wanted: Vec<usize> = (0..prepared.len()).filter(|&i| prepared.get(i).is_some_and(|p| is_translatable(&p.block.text))).collect();
    if wanted.is_empty() {
        entry.skipped = Some(SkipReason::NoText);
        return Ok(());
    }
    let segments: Vec<String> = wanted.iter().filter_map(|&i| prepared.get(i).map(|p| p.source.clone())).collect();
    let mut done: Vec<String> = Vec::with_capacity(segments.len());
    for batch in batches(&segments, opts.batch_chars) {
        if cancel.load(Ordering::Relaxed) {
            entry.skipped = Some(SkipReason::Cancelled);
            return Ok(());
        }
        match send(tr, language, batch, opts.retries, cancel) {
            Ok(mut out) => done.append(&mut out),
            Err(_) if cancel.load(Ordering::Relaxed) => {
                entry.skipped = Some(SkipReason::Cancelled);
                return Ok(());
            }
            Err(e) => return Err(e),
        }
    }
    if cancel.load(Ordering::Relaxed) {
        entry.skipped = Some(SkipReason::Cancelled);
        return Ok(());
    }
    let mut translated: Vec<Option<String>> = vec![None; prepared.len()];
    for (&i, text) in wanted.iter().zip(done) {
        if let Some(slot) = translated.get_mut(i) {
            *slot = Some(text);
        }
    }
    let edits: Vec<BlockEdit> = prepared
        .iter()
        .zip(translated)
        .map(|(p, text)| {
            let key = BlockKey::of(&p.block);
            let Some(text) = text else { return BlockEdit::new(key, p.block.text.clone()) };
            match &p.marked {
                // Only hard breaks to restore: the block keeps its own single style.
                Some(m) if m.runs.is_empty() => BlockEdit::new(key, untag_runs(&text, &m.runs).text),
                Some(m) => {
                    let done = untag_runs(&text, &m.runs);
                    BlockEdit { key, text: done.text, style: done.exact, fallback: Some(done.standard) }
                }
                None => BlockEdit::new(key, text),
            }
        })
        .collect();
    let min_scale = if opts.min_scale.is_finite() { opts.min_scale.clamp(0.7, 1.0) } else { 0.7 };
    let batch = rewrite_page_blocks(doc, page, &edits, min_scale).map_err(|e| format!("could not rewrite the page: {e}"))?;
    for outcome in &batch.outcomes {
        match outcome {
            BlockOutcome::Applied { substituted, overflow, plain, .. } => {
                entry.applied += 1;
                entry.substituted += usize::from(substituted.is_some());
                entry.overflowed += usize::from(*overflow);
                entry.plain += usize::from(*plain);
            }
            BlockOutcome::Unchanged => entry.unchanged += 1,
            BlockOutcome::Merges => entry.merged += 1,
            BlockOutcome::Lost { .. } => entry.lost += 1,
            BlockOutcome::Missing | BlockOutcome::Failed(_) => entry.failed += 1,
        }
    }
    Ok(())
}

/// Calls the translator, trying again when it fails or answers with the wrong number of segments.
fn send(tr: &dyn Translator, language: &str, batch: &[String], retries: usize, cancel: &AtomicBool) -> Result<Vec<String>, String> {
    let mut last = String::new();
    for _ in 0..=retries {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        match tr.translate(language, batch, cancel) {
            Ok(out) if out.len() == batch.len() => return Ok(out),
            Ok(out) => last = format!("the translator returned {} segments and {} were sent", out.len(), batch.len()),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Consecutive groups of segments of up to `limit` characters (a bigger segment goes alone).
fn batches(segments: &[String], limit: usize) -> Vec<&[String]> {
    let mut out = Vec::new();
    let (mut start, mut chars) = (0usize, 0usize);
    for (i, s) in segments.iter().enumerate() {
        let n = s.chars().count();
        if i > start && chars.saturating_add(n) > limit {
            if let Some(group) = segments.get(start..i) {
                out.push(group);
            }
            start = i;
            chars = 0;
        }
        chars = chars.saturating_add(n);
    }
    if let Some(group) = segments.get(start..)
        && !group.is_empty()
    {
        out.push(group);
    }
    out
}

#[cfg(test)]
#[path = "translate_tests.rs"]
mod tests;
