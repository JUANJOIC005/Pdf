use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use super::*;

/// Prefixes each segment with "[es] " (marks stay where they are). Can fail, miscount or cancel.
#[derive(Default)]
struct Fake {
    /// Calls that fail before one succeeds.
    fail: AtomicUsize,
    /// Calls that answer with one segment too few.
    short: AtomicUsize,
    /// Sets `cancel` on the n-th call (1-based); 0 never.
    cancel_on: usize,
    /// Segment count of every call.
    calls: Mutex<Vec<usize>>,
}

impl Translator for Fake {
    fn translate(&self, _: &str, segments: &[String], cancel: &AtomicBool) -> Result<Vec<String>, String> {
        let n = {
            let mut calls = self.calls.lock().map_err(|e| e.to_string())?;
            calls.push(segments.len());
            calls.len()
        };
        if self.cancel_on == n {
            cancel.store(true, Ordering::Relaxed);
        }
        if self.fail.load(Ordering::Relaxed) > 0 {
            self.fail.fetch_sub(1, Ordering::Relaxed);
            return Err("boom".into());
        }
        let mut out: Vec<String> = segments.iter().map(|s| format!("[es] {s}")).collect();
        if self.short.load(Ordering::Relaxed) > 0 {
            self.short.fetch_sub(1, Ordering::Relaxed);
            out.pop();
        }
        Ok(out)
    }
}

impl Fake {
    fn calls(&self) -> Vec<usize> {
        self.calls.lock().map(|c| c.clone()).unwrap_or_default()
    }
}

/// A document with one page per content string (Helvetica as /F1).
fn doc_of(contents: &[&str]) -> Document {
    let kids: Vec<String> = (0..contents.len()).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut objs: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), contents.len()),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),
    ];
    for (i, c) in contents.iter().enumerate() {
        objs.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>",
            5 + 2 * i
        ));
        objs.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()));
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).expect("synthetic document opens")
}

const TWO: &str = "BT /F1 12 Tf 72 700 Td (First paragraph) Tj ET BT /F1 12 Tf 72 600 Td (Second paragraph) Tj ET";
const ONE: &str = "BT /F1 12 Tf 72 700 Td (Only paragraph) Tj ET";
const DIGITS: &str = "BT /F1 12 Tf 72 700 Td (12345) Tj ET";

fn page_text(doc: &Document, page: usize) -> String {
    text_blocks(doc, page).map(|b| b.iter().map(|b| b.text.clone()).collect::<Vec<_>>().join("|")).unwrap_or_default()
}

fn run(doc: &mut Document, pages: &[usize], opts: &TranslateOptions, tr: &Fake, cancel: &AtomicBool) -> (TranslationReport, Vec<Progress>) {
    let mut events = Vec::new();
    let report = translate_document(doc, pages, "es", opts, tr, &mut |p| events.push(p), cancel);
    (report, events)
}

#[test]
fn translates_several_pages_and_reports_progress() {
    let mut doc = doc_of(&[TWO, ONE]);
    let tr = Fake::default();
    let (report, events) = run(&mut doc, &[0, 1], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert!(!report.cancelled);
    assert_eq!(report.pages.iter().map(|p| p.applied).collect::<Vec<_>>(), [2, 1]);
    assert!(report.pages.iter().all(|p| p.error.is_none() && p.skipped.is_none()));
    assert!(page_text(&doc, 0).contains("[es] First paragraph"), "{}", page_text(&doc, 0));
    assert!(page_text(&doc, 1).contains("[es] Only paragraph"));
    assert_eq!(events, [Progress::Started { pages: 2 }, Progress::Page { done: 1, of: 2 }, Progress::Page { done: 2, of: 2 }]);
}

#[test]
fn a_page_without_text_is_skipped_and_nothing_is_sent() {
    let mut doc = doc_of(&["", DIGITS]);
    let tr = Fake::default();
    let (report, _) = run(&mut doc, &[0, 1], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert!(report.pages.iter().all(|p| p.skipped == Some(SkipReason::NoText) && p.error.is_none()));
    assert!(tr.calls().is_empty());
}

#[test]
fn batches_are_split_by_characters() {
    let mut doc = doc_of(&[TWO]);
    let tr = Fake::default();
    let opts = TranslateOptions { batch_chars: 20, ..TranslateOptions::default() };
    let (report, _) = run(&mut doc, &[0], &opts, &tr, &AtomicBool::new(false));
    assert_eq!(tr.calls(), [1, 1]);
    assert_eq!(report.pages.first().map(|p| p.applied), Some(2));
    let tr = Fake::default();
    let mut doc = doc_of(&[TWO]);
    run(&mut doc, &[0], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert_eq!(tr.calls(), [2]);
}

#[test]
fn batching_groups_without_losing_segments() {
    let s: Vec<String> = ["aaaa", "bb", "cccccccccc", "d"].iter().map(|s| s.to_string()).collect();
    let sizes: Vec<usize> = batches(&s, 6).iter().map(|b| b.len()).collect();
    assert_eq!(sizes, [2, 1, 1]);
    assert!(batches(&[], 6).is_empty());
}

#[test]
fn a_failed_batch_is_retried() {
    let mut doc = doc_of(&[ONE]);
    let tr = Fake { fail: AtomicUsize::new(1), ..Fake::default() };
    let (report, _) = run(&mut doc, &[0], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert_eq!(tr.calls().len(), 2);
    assert_eq!(report.pages.first().map(|p| (p.applied, p.error.clone())), Some((1, None)));
    // A wrong count is retried the same way.
    let mut doc = doc_of(&[TWO]);
    let tr = Fake { short: AtomicUsize::new(1), ..Fake::default() };
    let (report, _) = run(&mut doc, &[0], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert_eq!(tr.calls().len(), 2);
    assert_eq!(report.pages.first().map(|p| p.applied), Some(2));
}

#[test]
fn a_persistent_failure_marks_the_page_and_goes_on() {
    let mut doc = doc_of(&[ONE, TWO]);
    // Both attempts for page 1 fail; page 2 then succeeds.
    let tr = Fake { fail: AtomicUsize::new(2), ..Fake::default() };
    let (report, _) = run(&mut doc, &[0, 1], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert_eq!(report.pages.first().and_then(|p| p.error.as_deref()), Some("boom"));
    assert_eq!(report.pages.first().map(|p| p.applied), Some(0));
    assert_eq!(report.pages.get(1).map(|p| (p.applied, p.error.clone())), Some((2, None)));
    assert_eq!(page_text(&doc, 0), "Only paragraph");
    assert!(page_text(&doc, 1).contains("[es] "));
}

#[test]
fn a_wrong_count_every_time_is_an_error() {
    let mut doc = doc_of(&[TWO]);
    let tr = Fake { short: AtomicUsize::new(9), ..Fake::default() };
    let opts = TranslateOptions { retries: 0, ..TranslateOptions::default() };
    let (report, _) = run(&mut doc, &[0], &opts, &tr, &AtomicBool::new(false));
    assert!(report.pages.first().and_then(|p| p.error.as_deref()).is_some_and(|e| e.contains("1 segments and 2")));
    assert_eq!(tr.calls().len(), 1);
}

#[test]
fn cancelling_keeps_the_pages_already_done() {
    let mut doc = doc_of(&[ONE, ONE, ONE]);
    let tr = Fake { cancel_on: 2, ..Fake::default() };
    let cancel = AtomicBool::new(false);
    let (report, events) = run(&mut doc, &[0, 1, 2], &TranslateOptions::default(), &tr, &cancel);
    assert!(report.cancelled);
    assert_eq!(report.pages.first().map(|p| p.applied), Some(1));
    assert_eq!(report.pages.get(1).and_then(|p| p.skipped), Some(SkipReason::Cancelled));
    assert_eq!(report.pages.get(2).and_then(|p| p.skipped), Some(SkipReason::Cancelled));
    assert!(page_text(&doc, 0).contains("[es] "));
    assert_eq!(page_text(&doc, 1), "Only paragraph");
    assert_eq!(tr.calls().len(), 2);
    assert_eq!(events.len(), 4);
}

#[test]
fn already_cancelled_sends_nothing() {
    let mut doc = doc_of(&[ONE]);
    let tr = Fake::default();
    let (report, _) = run(&mut doc, &[0], &TranslateOptions::default(), &tr, &AtomicBool::new(true));
    assert!(report.cancelled && tr.calls().is_empty());
}

#[test]
fn invalid_and_repeated_pages_are_reported_not_fatal() {
    let mut doc = doc_of(&[ONE]);
    let tr = Fake::default();
    let (report, _) = run(&mut doc, &[0, 0, 7, usize::MAX], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert_eq!(report.pages.len(), 4);
    assert_eq!(report.pages.first().map(|p| p.applied), Some(1));
    assert!(report.pages.get(1).and_then(|p| p.error.as_deref()).is_some_and(|e| e.contains("repeated")));
    assert!(report.pages.get(2).and_then(|p| p.error.as_deref()).is_some_and(|e| e.contains("out of range")));
    assert!(report.pages.get(3).is_some_and(|p| p.error.is_some()));
    assert_eq!(tr.calls().len(), 1);
}

#[test]
fn a_document_without_pages_is_fine() {
    let mut doc = doc_of(&[]);
    let tr = Fake::default();
    let (report, events) = run(&mut doc, &[], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert!(report.pages.is_empty() && !report.cancelled);
    assert_eq!(events, [Progress::Started { pages: 0 }]);
    let (report, _) = run(&mut doc, &[0], &TranslateOptions::default(), &tr, &AtomicBool::new(false));
    assert!(report.pages.first().and_then(|p| p.error.as_deref()).is_some());
}

#[test]
fn code_detection() {
    assert!(looks_like_code("a = [1, (2)]; \"x\""));
    assert!(!looks_like_code("Plain prose, (one aside)."));
}
