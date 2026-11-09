//! Pilot: translate the text of one page in place, keeping its layout.
//!
//! `cargo run -p pdfcraft-edit --example translate_page -- IN.pdf PAGE OUT.pdf [--lang español] [--provider codex|claude] [--mock]`
//!
//! A thin wrapper over `translate_document`: the paragraphs of the page go to the model and their translation
//! replaces them, keeping font, colour and position, re-wrapped in the same frame and shrunk when it doesn't fit. `--mock` skips the model and prefixes the text, to
//! check the rewriting alone. The input file is never modified.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use pdfcraft_ai::Provider;
use pdfcraft_cos::{Document, SaveOptions, write_full};
use pdfcraft_edit::text::{TranslateOptions, Translator, translate_document};

/// Sends the segments to the model, or just prefixes them with `--mock`.
struct Ai {
    provider: Provider,
    mock: bool,
}

impl Translator for Ai {
    fn translate(&self, language: &str, segments: &[String], _cancel: &AtomicBool) -> Result<Vec<String>, String> {
        if self.mock {
            Ok(segments.iter().map(|s| format!("[{language}] {s}")).collect())
        } else {
            pdfcraft_ai::translate_segments(self.provider, language, segments)
        }
    }
}

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(page), Some(output)) = (args.next(), args.next(), args.next()) else {
        return Err("uso: translate_page IN.pdf PAGE OUT.pdf [--lang español] [--provider codex|claude] [--mock]".into());
    };
    let page: usize = page.parse::<usize>().ok().and_then(|p| p.checked_sub(1)).ok_or("PAGE es un número desde 1")?;
    let (mut language, mut provider, mut mock) = ("español".to_string(), Provider::Claude, false);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--lang" => language = args.next().ok_or("--lang necesita un idioma")?,
            "--provider" => provider = args.next().as_deref().and_then(Provider::parse).ok_or("--provider: codex o claude")?,
            "--mock" => mock = true,
            other => return Err(format!("opción desconocida: {other}")),
        }
    }
    let bytes = std::fs::read(&input).map_err(|e| format!("no se pudo leer {input}: {e}"))?;
    let mut doc = Document::open(Arc::new(bytes)).map_err(|e| format!("no se pudo abrir el PDF: {e}"))?;
    let cancel = AtomicBool::new(false);
    let report =
        translate_document(&mut doc, &[page], &language, &TranslateOptions::default(), &Ai { provider, mock }, &mut |p| eprintln!("{p:?}"), &cancel);
    for p in &report.pages {
        eprintln!("página {}: {p:?}", p.page + 1);
        if let Some(e) = &p.error {
            return Err(e.clone());
        }
    }
    let out = write_full(&doc, &SaveOptions::default()).map_err(|e| format!("no se pudo guardar: {e}"))?;
    std::fs::write(&output, out).map_err(|e| format!("no se pudo escribir {output}: {e}"))?;
    eprintln!("escrito {output}");
    Ok(())
}
