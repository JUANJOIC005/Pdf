//! `text_translate`: the offline `echo` provider writes a translated copy inside `--root` and
//! never touches the open document; paths outside the root, the input file itself and the
//! providers that call an external agent are refused.

use std::path::PathBuf;

use pdfcraft_automation::{Automation, Content};
use serde_json::{Value, json};

/// A two-page PDF; page `i` reads "Hello page i".
fn pdf() -> Vec<u8> {
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [4 0 R 6 0 R] /Count 2 /MediaBox [0 0 300 300] >>".to_vec(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ];
    for i in 0..2 {
        objs.push(format!("<< /Type /Page /Parent 2 0 R /Contents {} 0 R /Resources << /Font << /F1 3 0 R >> >> >>", 5 + 2 * i).into_bytes());
        let body = format!("BT /F1 18 Tf 20 150 Td (Hello page {}) Tj ET", i + 1);
        objs.push(format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()).into_bytes());
    }
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
    out
}

/// A root folder with `a.pdf` and a sibling `outside` folder; the tool runs confined to the root.
fn setup(tag: &str, allow_ai: bool) -> (Automation, PathBuf, u64) {
    let base = std::env::temp_dir().join(format!("pdfcraft-translate-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("root")).unwrap();
    std::fs::create_dir_all(base.join("outside")).unwrap();
    std::fs::write(base.join("root/a.pdf"), pdf()).unwrap();
    let mut a = Automation::new().with_root(base.join("root")).unwrap().with_allow_ai(allow_ai);
    let opened = json_of(a.call("doc_open", &json!({ "path": "a.pdf" })).unwrap());
    let id = opened["doc"].as_u64().unwrap();
    (a, base, id)
}

fn json_of(mut out: Vec<Content>) -> Value {
    match out.remove(0) {
        Content::Json(v) => v,
        other => panic!("expected JSON, got {other:?}"),
    }
}

fn text_of(path: &std::path::Path, page: u64) -> String {
    let mut a = Automation::new();
    let id = json_of(a.call("doc_open", &json!({ "path": path.to_str().unwrap() })).unwrap())["doc"].as_u64().unwrap();
    json_of(a.call("text_extract", &json!({ "doc": id, "pages": [page] })).unwrap()).to_string()
}

#[test]
fn echo_writes_a_translated_copy_inside_the_root_and_leaves_the_document_open_unchanged() {
    let (mut a, base, id) = setup("echo", false);
    let r = json_of(a.call("text_translate", &json!({ "doc": id, "pages": "2", "language": "es", "out": "es.pdf" })).unwrap());
    assert_eq!(r["applied"], 1, "{r}");
    assert_eq!(r["pages"][0]["page"], 2);
    assert_eq!(r["signed"], false);
    let written = base.join("root/es.pdf");
    assert!(text_of(&written, 2).contains("[es] Hello page 2"));
    assert!(!text_of(&written, 1).contains("[es]"), "pages outside the range are untouched");
    // The open document keeps its bytes and state; the input file on disk is unchanged too.
    assert_eq!(std::fs::read(base.join("root/a.pdf")).unwrap(), pdf());
    let listed = json_of(a.call("doc_list", &json!({})).unwrap());
    assert_eq!(listed["documents"][0]["dirty"], false, "{listed}");
    let again = json_of(a.call("text_extract", &json!({ "doc": id })).unwrap());
    assert!(!again.to_string().contains("[es]"), "{again}");
}

#[test]
fn an_output_outside_the_root_is_refused_and_nothing_is_written() {
    let (mut a, base, id) = setup("confine", false);
    let abs = base.join("outside/x.pdf");
    for out in ["../outside/x.pdf", abs.to_str().unwrap(), "sub/../../outside/x.pdf"] {
        let e = a.call("text_translate", &json!({ "doc": id, "language": "es", "out": out })).unwrap_err();
        assert!(e.to_string().contains("outside"), "{out}: {e}");
    }
    assert!(!base.join("outside/x.pdf").exists());
}

#[test]
fn the_output_may_not_be_the_input_file() {
    let (mut a, base, id) = setup("same", false);
    let e = a.call("text_translate", &json!({ "doc": id, "language": "es", "out": "a.pdf" })).unwrap_err();
    assert!(e.to_string().contains("input"), "{e}");
    let e = a.call("text_translate", &json!({ "doc": id, "language": "es", "out": "./sub/../a.pdf" })).unwrap_err();
    assert!(e.to_string().contains("input"), "{e}");
    assert_eq!(std::fs::read(base.join("root/a.pdf")).unwrap(), pdf());
}

#[test]
fn ai_providers_need_allow_ai_however_the_tool_is_reached() {
    let (mut a, base, id) = setup("ai", false);
    for provider in ["codex", "claude"] {
        let args = json!({ "doc": id, "language": "es", "provider": provider, "out": "ai.pdf" });
        let e = a.call("text_translate", &args).unwrap_err();
        assert!(e.to_string().contains("allow_ai"), "{provider}: {e}");
    }
    assert!(!base.join("root/ai.pdf").exists());
    let e = a.call("text_translate", &json!({ "doc": id, "language": "es", "provider": "gpt", "out": "ai.pdf" })).unwrap_err();
    assert!(e.to_string().contains("unknown provider"), "{e}");
}

#[test]
fn bad_ranges_and_scales_are_refused() {
    let (mut a, base, id) = setup("bad", false);
    for (key, value) in [("pages", json!("9")), ("pages", json!("x")), ("min_scale", json!(0.1)), ("language", json!(" "))] {
        let mut args = json!({ "doc": id, "language": "es", "out": "bad.pdf" });
        args[key] = value;
        assert!(a.call("text_translate", &args).is_err(), "{args}");
    }
    assert!(!base.join("root/bad.pdf").exists());
}
