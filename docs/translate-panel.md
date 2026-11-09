# Translation panel

Status: added 2026-11-09. Translates pages of the open PDF into a **new copy**; the open document, its undo history and its "modified" flag are never touched. The engine is described in `translate-in-place.md`; this file is the panel that drives it.

## Flow (`crates/ui-egui/src/translate_ui.rs`)
1. Pick the target language (ten one-tap choices, or "Other…" and type its name; the English name is what the agent receives) and the pages (current page, all pages, or a range such as `1-3, 5`).
2. "Start translation" takes the bytes the document would save now (so unsaved edits are translated too), opens them in a private `Session` in a worker thread and runs `pdfcraft_engine::translate::translate_copy`. Progress shows as "page n of N" with a Cancel button; cancelling leaves no output.
3. The result card says how many pages were translated, pages without text, paragraphs that could not be translated or are tight for their box, and warns that the copy does not keep signatures. "Open copy" opens it as a new tab; "Save as…" writes it (`Report (Spanish).pdf`).

## Decisions
- The agent and model are the AI panel's choice (`ai_model::picker`, `PdfCraftApp::ai_choice`): one setting, one widget.
- Refused before anything is sent: a document that forbids copying its text, an invalid page range, an empty language.
- Tests replace the translator through `PdfCraftApp::set_translator_factory` (the offline `EchoTranslator`), so no agent runs.
- Desktop only: on the web the panel is a stub (`translate_panel` in `lib.rs`).
- The rail gained a "Translate" button right after the AI assistant (`chrome.rs`); `panel=translate` opens it through the control channel.
