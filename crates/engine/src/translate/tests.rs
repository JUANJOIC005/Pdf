use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pdfcraft_edit::text::{Progress, SkipReason, TranslateOptions};

use super::*;
use crate::tests::{fixture, protected};

fn open(bytes: Arc<Vec<u8>>) -> (Session, DocId) {
    let mut s = Session::new();
    let id = s.open("t.pdf", None, bytes, None).unwrap();
    (s, id)
}

fn run(s: &Session, id: DocId, pages: &[usize]) -> Result<TranslationOutput, EditError> {
    translate_copy(s, id, pages, "es", &TranslateOptions::default(), &EchoTranslator, &mut |_: Progress| {}, &AtomicBool::new(false))
}

fn page_text(bytes: Vec<u8>, page: usize) -> String {
    let doc = pdfcraft_cos::Document::open(Arc::new(bytes)).unwrap();
    pdfcraft_edit::text::text_blocks(&doc, page).unwrap().into_iter().map(|b| b.text).collect::<Vec<_>>().join(" ")
}

#[test]
fn translating_a_copy_marks_the_text_and_leaves_the_open_document_alone() {
    let (s, id) = open(Arc::new(fixture(3)));
    let before = s.get(id).unwrap().bytes.clone();
    let saved_before = s.save_bytes(id).unwrap();
    let out = run(&s, id, &[0, 2]).unwrap();
    assert!(!out.signed);
    assert_eq!(out.report.pages.iter().filter(|p| p.applied > 0).count(), 2, "{:?}", out.report);
    assert!(page_text(out.bytes.clone(), 0).starts_with("[es] "), "{}", page_text(out.bytes.clone(), 0));
    assert!(page_text(out.bytes.clone(), 2).starts_with("[es] "));
    assert_eq!(page_text(out.bytes, 1), "Page 2", "a page that was not chosen is untouched");
    // The session's document: same bytes, same state, no edit to undo.
    let d = s.get(id).unwrap();
    assert!(!d.dirty);
    assert_eq!(*d.bytes, *before);
    assert_eq!(*s.save_bytes(id).unwrap(), *saved_before);
    assert_eq!(d.text_blocks(0).first().map(|b| b.text.as_str()), Some("Page 1"));
}

#[test]
fn a_document_that_cannot_be_modified_or_copied_is_refused_before_anything_is_sent() {
    struct Spy<'a>(&'a std::cell::Cell<bool>);
    impl Translator for Spy<'_> {
        fn translate(&self, _: &str, s: &[String], _: &AtomicBool) -> Result<Vec<String>, String> {
            self.0.set(true);
            Ok(s.to_vec())
        }
    }
    for permissions in [-1 ^ 0b1000, -1 ^ 0b1_0000] {
        let (s, id) = open(protected("", "owner", permissions));
        let sent = std::cell::Cell::new(false);
        let r = translate_copy(&s, id, &[0], "es", &TranslateOptions::default(), &Spy(&sent), &mut |_| {}, &AtomicBool::new(false));
        assert!(matches!(r, Err(EditError::NotPermitted(_))), "{permissions}: {r:?}");
        assert!(!sent.get());
    }
}

#[test]
fn an_encrypted_document_stays_encrypted_in_the_copy() {
    let (s, id) = open(protected("", "owner", -1));
    let out = run(&s, id, &[0]).unwrap();
    assert!(pdfcraft_cos::Document::open(Arc::new(out.bytes.clone())).unwrap().output_handler().is_some());
}

#[test]
fn bad_requests_are_refused() {
    let (s, id) = open(Arc::new(fixture(3)));
    assert!(matches!(run(&s, id, &[]), Err(EditError::Invalid(_))));
    assert!(matches!(run(&s, id, &[3]), Err(EditError::Invalid(_))));
    assert!(matches!(run(&s, DocId(9999), &[0]), Err(EditError::NoDocument)));
    let many: Vec<usize> = std::iter::repeat_n(0, MAX_TRANSLATE_PAGES + 1).collect();
    assert!(matches!(run(&s, id, &many), Err(EditError::Invalid(m)) if m.contains("200")));
    let blank = translate_copy(&s, id, &[0], " ", &TranslateOptions::default(), &EchoTranslator, &mut |_| {}, &AtomicBool::new(false));
    assert!(matches!(blank, Err(EditError::Invalid(_))));
}

#[test]
fn cancelling_gives_no_output() {
    let (s, id) = open(Arc::new(fixture(3)));
    let r = translate_copy(&s, id, &[0, 1], "es", &TranslateOptions::default(), &EchoTranslator, &mut |_| {}, &AtomicBool::new(true));
    assert_eq!(r.unwrap_err(), EditError::Cancelled);
    assert!(!s.get(id).unwrap().dirty);
}

#[test]
fn pages_without_text_are_reported_not_failed() {
    let text = String::from_utf8_lossy(&fixture(1)).replace("(Page 1) Tj", "() Tj");
    let (s, id) = open(Arc::new(text.into_bytes()));
    let out = run(&s, id, &[0]).unwrap();
    assert_eq!(out.report.pages[0].skipped, Some(SkipReason::NoText));
}

#[test]
fn page_ranges() {
    assert_eq!(parse_page_range("all", 4).unwrap(), vec![0, 1, 2, 3]);
    assert_eq!(parse_page_range("3", 4).unwrap(), vec![2]);
    assert_eq!(parse_page_range("2-3", 4).unwrap(), vec![1, 2]);
    assert_eq!(parse_page_range("3,1,2-3, 1", 4).unwrap(), vec![0, 1, 2]);
    assert_eq!(parse_page_range("4-2", 4).unwrap(), vec![1, 2, 3]);
    for bad in ["", "0", "5", "x", "1-9", "99999999999999999999"] {
        assert!(parse_page_range(bad, 4).is_err(), "{bad:?}");
    }
    assert!(parse_page_range("all", 0).is_err());
    assert!(parse_page_range("all", MAX_SPEC_PAGES + 1).is_err());
}

#[test]
fn the_echo_translator_keeps_marks_and_watches_cancel() {
    let segs = vec!["⟦1⟧a⟦/1⟧⟦br⟧b".to_string()];
    assert_eq!(EchoTranslator.translate("fr", &segs, &AtomicBool::new(false)).unwrap(), vec!["[fr] ⟦1⟧a⟦/1⟧⟦br⟧b".to_string()]);
    assert!(EchoTranslator.translate("fr", &segs, &AtomicBool::new(true)).is_err());
}
