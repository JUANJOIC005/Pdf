//! The Markdown renderer shows a table, a list, code with a Copy button and a link, in both
//! themes, and hands a clicked link back to its caller instead of opening it.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use pdfcraft_ui_egui::markdown::{self, MarkdownCache};
use pdfcraft_ui_egui::theme::{ThemeKind, Tokens};

const DOC: &str = "# Title\n\nSome **bold** text with a [site](https://example.org) and `code`, plus $\\alpha^2$.\n\n\
| Name | Qty |\n|:-----|----:|\n| Alpha | 12 |\n| Beta | 7 |\n\n\
1. first\n2. second\n   - nested\n\n> quoted words\n\n```rust\nlet x = \"hi\"; // c\n```\n\n<script>x</script>\n";

fn harness(kind: ThemeKind) -> (Harness<'static, Option<String>>, ()) {
    let t = Tokens::for_kind(kind);
    let mut cache = MarkdownCache::default();
    let mut fonts = false;
    let mut h = Harness::builder().with_size(egui::vec2(600.0, 900.0)).build_ui_state(
        move |ui, clicked: &mut Option<String>| {
            if !std::mem::replace(&mut fonts, true) {
                egui_extras::install_image_loaders(ui.ctx());
                pdfcraft_ui_egui::theme::install_fonts(ui.ctx());
            }
            let blocks = cache.get(DOC);
            if let Some(u) = markdown::show(ui, &blocks, &t, egui::Id::unique("md")) {
                *clicked = Some(u);
            }
        },
        None,
    );
    h.run();
    (h, ())
}

#[test]
fn renders_in_both_themes() {
    for kind in [ThemeKind::Light, ThemeKind::Dark] {
        let (h, ()) = harness(kind);
        h.get_by_label("Alpha");
        h.get_by_label("Beta");
        h.get_by_label("Copy");
        h.get_by_label("first");
        h.get_by_label("quoted words");
    }
}

#[test]
fn clicked_link_is_returned() {
    let (mut h, ()) = harness(ThemeKind::Light);
    h.get_by_label("site").click();
    h.run();
    assert_eq!(h.state().as_deref(), Some("https://example.org"));
}
