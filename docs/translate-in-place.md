# Translating a page in place (pilot)

> **Last updated:** 2026-11-09 · Status: pilot. A headless example, not yet in the AI panel.

`cargo run -p pdfcraft-edit --example translate_page -- IN.pdf PAGE OUT.pdf [--lang español] [--provider codex|claude] [--mock]`
writes a copy of one page with its text translated where it was, keeping font, size, colour
and position. The input is never modified. `--mock` skips the model and prefixes each paragraph,
to check the rewriting alone.

## How it works

1. `text_blocks` lists the page's paragraphs. A paragraph made only of numbers or symbols
   (`is_translatable`) is not sent and stays as it is.
2. Prose goes to the model as one flowing line (its row breaks only reflect the old width).
   `tag_runs` marks stretches of one style as `⟦n⟧…⟦/n⟧` and hard line breaks (list items, short
   sentence-ending rows, a size change) as `⟦br⟧`. The model keeps the marks around the equivalent
   words (`crates/ai`, `translate_segments`: one JSON array in, one translation per segment out).
3. `untag_runs` removes the marks and builds the style ranges. `rewrite_page_blocks`
   (`crates/edit/src/text/batch.rs`) rewrites each paragraph.

## Why the batch looks paragraphs up by key

A paragraph's index is its position in a list regrouped from geometry on every read, so rewriting
one paragraph can merge or split the others. Each edit names its paragraph by `BlockKey`, is tried
on a copy, and is kept only when every paragraph still to be edited can still be found. An edit
that would merge paragraphs waits until the others are done; one that never can is reported
(`Merges`), not forced.

## What each edit reports (`BlockOutcome`)

- `Applied { scale, overflow, plain, substituted }`: `scale` is the font size relative to the
  original (it shrinks, down to the minimum given, until the text fits its box, the page's right
  edge and its neighbours); `overflow` means it still did not fit; `plain` means the styles asked
  for could not be drawn; `substituted` names a stand-in font.
- `Unchanged`, `Missing` (no paragraph has the key), `Merges`, `Failed`, and `Lost { missing }`:
  the new text would lose characters of the requested one, so nothing was written.

Styles are tried in three tiers: the run's own font resource, then the nearest standard family
(bold and italic kept) with the run's size and colour, then the paragraph's own style.

## Known limits

- Fonts that are subsets lack letters the translation needs; those runs fall back to a standard
  font. `has_glyph` (`crates/fonts/src/pdf.rs`) now checks the embedded program for CID TrueType
  subsets, not only `/W`.
- Underlines drawn as separate lines, link annotations, superscripts and diagram labels do not
  follow the translated text.
- Scanned pages, text as curves, slanted text, forms and signed or encrypted files are not handled.
- `missing_chars` does not normalise NFC/NFD.

Design decisions: replace the text (not overlay it), save as a new copy, adaptive font size,
retry what fails. Planned: a metrics harness over many PDFs, left/right/centre alignment per
block, moving underlines and links with their text, (the panel button is done: see `translate-panel.md`).

Revision history: 2026-11-09 — pilot added.
