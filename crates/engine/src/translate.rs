//! Translate pages of an open document into a new PDF, leaving the open document untouched.
//!
//! [`translate_copy`] takes the bytes the session would save, opens a private copy, runs
//! `pdfcraft_edit::text::translate_document` on it and writes the copy in full. The session's
//! document, its undo history and its "modified" flag are never touched.
//!
//! **Encryption.** `write_full` writes with the security handler the copy was opened with, so an
//! encrypted document stays encrypted with the same passwords and permissions. A copy that ends up
//! without a handler although the source had one is refused rather than returned in the clear.
//!
//! **Signatures.** A full rewrite cannot keep a signature valid. The result says so
//! ([`TranslationOutput::signed`]) so the caller can warn; the signed original is not modified.

use std::sync::atomic::{AtomicBool, Ordering};

use pdfcraft_ai::{AiError, Options};
use pdfcraft_edit::text::translate_document;

pub use pdfcraft_edit::text::{PageReport, SkipReason, TranslateOptions, Translator, Progress, TranslationReport};

use crate::doc_state::guard;
use crate::{DocId, EditError, Session};

/// Most pages one call translates (a bigger range is refused before anything is sent).
pub const MAX_TRANSLATE_PAGES: usize = 200;

/// Most pages a page-range spec may name.
const MAX_SPEC_PAGES: usize = 10_000;

/// The translated copy and what happened to each page.
#[derive(Debug)]
pub struct TranslationOutput {
    /// The translated PDF, written in full.
    pub bytes: Vec<u8>,
    pub report: TranslationReport,
    /// The source is signed: the copy does not keep valid signatures.
    pub signed: bool,
}

/// Translates through the `codex` or `claude` command-line login (`pdfcraft-ai`).
#[derive(Clone, Debug)]
pub struct AiTranslator {
    pub options: Options,
}

impl Translator for AiTranslator {
    fn translate(&self, language: &str, segments: &[String], cancel: &AtomicBool) -> Result<Vec<String>, String> {
        pdfcraft_ai::translate_segments_with(&self.options, language, segments, cancel).map_err(|e: AiError| e.to_string())
    }
}

/// Offline stand-in for tests and the `echo` provider: puts `[<language>] ` before each segment
/// and keeps every `⟦n⟧` / `⟦/n⟧` / `⟦br⟧` mark, which are part of the text.
#[derive(Clone, Copy, Debug, Default)]
pub struct EchoTranslator;

impl Translator for EchoTranslator {
    fn translate(&self, language: &str, segments: &[String], cancel: &AtomicBool) -> Result<Vec<String>, String> {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        Ok(segments.iter().map(|s| format!("[{language}] {s}")).collect())
    }
}

/// Translate `pages` (0-based) of the open document `id` into a new PDF.
///
/// Refused before anything is sent: no such document, no permission to modify or to copy,
/// no pages, more than [`MAX_TRANSLATE_PAGES`], a page out of range, an unreadable or
/// stream-limited document. Cancelling gives [`EditError::Cancelled`] and no output.
#[allow(clippy::too_many_arguments)]
pub fn translate_copy(
    session: &Session,
    id: DocId,
    pages: &[usize],
    language: &str,
    opts: &TranslateOptions,
    tr: &dyn Translator,
    progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<TranslationOutput, EditError> {
    let doc = session.get(id).ok_or(EditError::NoDocument)?;
    if !doc.allows_modification() {
        return Err(EditError::NotPermitted("changes to the document"));
    }
    doc.check_copying()?;
    if language.trim().is_empty() {
        return Err(EditError::Invalid("the target language is empty".into()));
    }
    if pages.is_empty() {
        return Err(EditError::Invalid("no pages to translate".into()));
    }
    if pages.len() > MAX_TRANSLATE_PAGES {
        return Err(EditError::Invalid(format!("at most {MAX_TRANSLATE_PAGES} pages can be translated at once (asked for {})", pages.len())));
    }
    let total = doc.info.pages.len();
    if let Some(p) = pages.iter().find(|p| **p >= total) {
        return Err(EditError::Invalid(format!("page {} is out of range: the document has {total} pages", p.saturating_add(1))));
    }
    let signed = doc.is_signed();
    let password = doc.password.clone();
    let bytes = session.save_bytes(id)?;
    let mut copy = pdfcraft_cos::Document::open_with_password(bytes, password.as_deref()).map_err(|e| EditError::Reopen(e.to_string()))?;
    let encrypted = copy.output_handler().is_some();
    let report = guard(|| translate_document(&mut copy, pages, language, opts, tr, progress, cancel)).map_err(EditError::Write)?;
    if report.cancelled || cancel.load(Ordering::Relaxed) {
        return Err(EditError::Cancelled);
    }
    if encrypted && copy.output_handler().is_none() {
        return Err(EditError::Write("the translated copy would lose the document's encryption".into()));
    }
    let out = guard(|| pdfcraft_cos::write_full(&copy, &pdfcraft_cos::SaveOptions::default())).map_err(EditError::Write)?.map_err(|e| EditError::Write(e.to_string()))?;
    Ok(TranslationOutput { bytes: out, report, signed })
}

/// Pages named by `spec` (`all`, `3`, `3-7`, `1,3,5-6`; 1-based) as sorted, distinct 0-based
/// indices. Uses the Print range parser (open-ended `5-` and `-3` allowed) so there is one
/// range grammar; descending ranges are put in order.
pub fn parse_page_range(spec: &str, page_count: usize) -> Result<Vec<usize>, String> {
    if page_count > MAX_SPEC_PAGES {
        return Err(format!("documents of more than {MAX_SPEC_PAGES} pages are not supported here"));
    }
    let spec = spec.trim();
    let range = if spec.eq_ignore_ascii_case("all") { None } else { Some(spec) };
    if range.is_some_and(str::is_empty) {
        return Err("the page range is empty".into());
    }
    let mut pages = pdfcraft_print::select_pages(page_count, range, &[], pdfcraft_print::Subset::All, false).map_err(|e| e.to_string())?;
    pages.sort_unstable();
    pages.dedup();
    if pages.is_empty() {
        return Err("the page range selects no pages".into());
    }
    Ok(pages)
}

#[cfg(test)]
mod tests;
