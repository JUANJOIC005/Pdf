# Continuous page display

> **Last updated:** 2026-11-09 · Viewer: force continuous scrolling.

To read pages as a continuous strip, open **View → Page display**, choose **Single page**,
turn **Continuous scrolling** on, and turn **Show gaps between pages** off. The Page display
button on the viewer rail opens the same choices. The command palette also offers
**Show gaps between pages** as a toggle.

Hiding gaps joins the displayed page edges and removes page shadows. It does not crop or
change the PDF: white margins already in a page remain. Pages of different widths stay
centered. In two-page view, it removes both the gutter within a row and the gap between rows.

For future documents, open **Preferences → Documents and view** and uncheck
**Show gaps between pages**. This default survives a restart. Existing documents retain their
current view; the per-document toggle does not change the preference for future documents.

Desktop launch options:

```sh
pdfcraft --layout continuous --page-gaps off document.pdf
pdfcraft --default-page-gaps off
```

The opt-in UI control channel accepts `set key=page-gaps value=off` for the active document
and `set key=default-page-gaps value=off` for future documents. Its `command` method accepts
`id=view.page_gaps`, and `state` reports `active.page_gaps` and `active.continuous`.

## Always open with continuous scrolling

Some PDFs ask for one page at a time (`/PageLayout /SinglePage`), and the app also remembers
the last view of each file. In single-page mode the scroll bar spans one page, not the
document. **Preferences → Documents and view → Always open documents with continuous
scrolling** (on by default since 0.19.3; turn it off to let each PDF choose) opens every document with continuous scrolling, over both the
PDF's request and the remembered view. It keeps the layout (single or two-up) and only turns
continuous scrolling on; the per-document View menu still switches it afterwards. It is a
separate switch on purpose: "Always use my default zoom and page layout" skips the PDF's
request but not the remembered view, which is applied after it (`persist/last_view.rs`).
The control channel accepts `set key=force-continuous value=on`; desktop option
`--force-continuous on|off`.

Revision history: 2026-10-17 — added local gapless-display controls and documentation. 2026-11-09 — added the force-continuous preference. 2026-11-09 — on by default.

In a side-by-side comparison, the gap setting applies to both panes. Changing it in
Organize Pages keeps your document scroll position when you return to the viewer.
