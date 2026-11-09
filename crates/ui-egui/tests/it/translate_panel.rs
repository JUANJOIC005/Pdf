//! The translation panel, driven through the offline echo translator so no agent runs: choosing
//! the language and the pages, translating, the result card, and opening the copy as a new tab.

use std::sync::Arc;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use pdfcraft_engine::Session;
use pdfcraft_engine::translate::EchoTranslator;
use pdfcraft_ui_egui::PdfCraftApp;

use crate::support;

fn harness() -> Harness<'static, PdfCraftApp> {
    let pdf = Session::new().create_from_text("t", "Delivery within five days.").unwrap().to_vec();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("t.pdf", None, pdf.clone()).unwrap();
        app.set_translator_factory(Arc::new(|_| Box::new(EchoTranslator)));
        app.set_option("panel", "translate").unwrap();
        app
    });
    h.run_steps(6);
    h
}

#[test]
fn translating_gives_a_copy_that_opens_in_a_new_tab() {
    let mut h = harness();
    h.get_by_label("French").click();
    h.run_steps(2);
    h.get_by_label("Start translation").click();
    assert!(support::step_until(&mut h, |h| h.query_by_label("Translation ready").is_some()), "the result card appears");
    h.get_by_label("1 of 1 pages translated");
    assert_eq!(h.state().views.len(), 1, "the original is the only document until the copy is opened");
    h.get_by_label("Open copy").click();
    assert!(support::step_until(&mut h, |h| h.state().views.len() == 2), "the copy opens as a new tab");
}

#[test]
fn a_bad_range_is_refused_before_anything_runs() {
    let mut h = harness();
    h.get_by_label("Range").click();
    h.run_steps(2);
    let input = h.get_all_by_role(egui::accesskit::Role::TextInput).last().unwrap();
    input.focus();
    input.type_text("nonsense");
    h.run_steps(2);
    h.get_by_label("Start translation").click();
    h.run_steps(4);
    h.get_by_label("That page range is not valid.");
    assert!(h.query_by_label("Translation ready").is_none());
}

#[test]
fn the_panel_never_names_the_preparing_tool() {
    let h = harness();
    let mut out = String::new();
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
    walk(&h.root(), &mut out);
    assert!(out.contains("Translate into"), "the walk reaches the panel");
    assert!(!out.to_lowercase().contains("markitdown"));
}
