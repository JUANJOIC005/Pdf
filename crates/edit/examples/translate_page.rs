//! Pilot: translate the text of one page in place, keeping its layout.
//!
//! `cargo run -p pdfcraft-edit --example translate_page -- IN.pdf PAGE OUT.pdf [--lang español] [--provider codex|claude] [--mock]`
//!
//! Each paragraph of the page (`text_blocks`) goes to the model as one segment and its translation
//! replaces the paragraph with `rewrite_page_blocks`, which finds each paragraph again by its key, keeps its font, colour and
//! position, re-wraps it in the same frame and shrinks the size when the text doesn't fit. `--mock` skips the model and prefixes the text, to
//! check the rewriting alone. The input file is never modified.

use std::sync::Arc;

use pdfcraft_ai::Provider;
use pdfcraft_cos::{Document, SaveOptions, write_full};
use pdfcraft_edit::text::{BlockEdit, BlockKey, rewrite_page_blocks, tag_runs, text_blocks, untag_runs};

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
    let blocks = text_blocks(&doc, page).map_err(|e| format!("no se pudo leer el texto de la página: {e}"))?;
    // Prose is sent as one flowing line: its breaks are only where the old width wrapped it. Code
    // and lists keep the rows they had. A paragraph that mixes styles is sent with its runs marked.
    let mut tagged = Vec::new();
    let mut sources: Vec<String> = Vec::new();
    for b in &blocks {
        let code = looks_like_code(&b.text);
        let marked = if code { None } else { tag_runs(&doc, page, b).map_err(|e| format!("no se pudieron leer los estilos: {e}"))? };
        sources.push(match &marked {
            Some(m) => m.text.clone(),
            None if code => b.reflow_text.clone(),
            None => b.text.clone(),
        });
        tagged.push(marked);
    }
    eprintln!("{} bloques de texto en la página {}", sources.len(), page + 1);
    let translated: Vec<String> = if mock {
        sources.iter().map(|s| format!("[{language}] {s}")).collect()
    } else {
        pdfcraft_ai::translate_segments(provider, &language, &sources)?
    };
    let edits: Vec<BlockEdit> = blocks
        .iter()
        .zip(translated.iter().zip(&tagged))
        .map(|(b, (text, marked))| match marked {
            Some(m) => {
                let done = untag_runs(text, &m.runs);
                BlockEdit { key: BlockKey::of(b), text: done.text, style: done.exact, fallback: Some(done.standard) }
            }
            None => BlockEdit::new(BlockKey::of(b), text.clone()),
        })
        .collect();
    let report = rewrite_page_blocks(&mut doc, page, &edits, 0.7).map_err(|e| format!("no se pudo reescribir la página: {e}"))?;
    for (index, (block, outcome)) in blocks.iter().zip(&report.outcomes).enumerate() {
        let shown: String = block.text.chars().take(40).collect();
        eprintln!("  [{index}] {outcome:?} {shown:?}");
    }
    let out = write_full(&doc, &SaveOptions::default()).map_err(|e| format!("no se pudo guardar: {e}"))?;
    std::fs::write(&output, out).map_err(|e| format!("no se pudo escribir {output}: {e}"))?;
    eprintln!("escrito {output}");
    Ok(())
}

/// Rows of code or data (assignments, brackets, quotes) must keep their line breaks.
fn looks_like_code(text: &str) -> bool {
    text.chars().filter(|c| matches!(c, '=' | '[' | ']' | '{' | '}' | '(' | ')' | '"' | ';')).count() >= 4
}
