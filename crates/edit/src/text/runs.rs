//! Style runs of a paragraph, carried through a translation.
//!
//! A paragraph can mix styles: a bold title over a body, a highlighted word in a sentence. Sent as
//! plain text it comes back in one style. [`tag_runs`] marks each stretch of one style as
//! `⟦n⟧…⟦/n⟧`; a translator keeps the marks around the equivalent words, and [`untag_runs`] turns
//! them back into [`RangeStyle`]s that draw those words in the font, size and colour of run `n`.

use super::{BlockStyle, EditError, TextBlock, text_lines};
use pdfcraft_cos::Document;
use std::ops::Range;

use super::blocks::RangeStyle;

/// The look of one run: the page font resource, the size and the fill colour of its source line.
#[derive(Clone, Debug, PartialEq)]
pub struct RunStyle {
    pub resource: String,
    pub size: f64,
    pub color: [f64; 3],
    pub bold: bool,
    pub italic: bool,
    /// The standard family closest to the run's font, for when its own can't show a letter.
    pub family: crate::added::Family,
}

/// A paragraph's text with its runs marked, and the style behind each mark (`⟦1⟧` is `runs[0]`).
#[derive(Clone, Debug, PartialEq)]
pub struct TaggedBlock {
    pub text: String,
    pub runs: Vec<RunStyle>,
}

const OPEN: char = '⟦';
const CLOSE: char = '⟧';

/// The literal mark for a hard line break inside the tagged text.
const BREAK: &str = "⟦br⟧";

/// A row boundary is a hard break when the next row opens with a list marker, or when the
/// previous row ends a sentence and is clearly short (under 70 % of the longest row, in
/// characters). A size change (decided by the caller) is a hard break too. Anything else was the
/// old width's wrap.
fn hard_breaks(editor_text: &str) -> Vec<bool> {
    let rows: Vec<&str> = editor_text.split('\n').collect();
    let widest = rows.iter().map(|r| r.trim().chars().count()).max().unwrap_or(0);
    rows.windows(2)
        .map(|pair| {
            let (prev, next) = (pair[0].trim(), pair[1].trim());
            let short = (prev.chars().count() as f64) < widest as f64 * 0.7;
            starts_with_list_marker(next) || (short && prev.ends_with(['.', ':', ';', '?', '!']))
        })
        .collect()
}

/// Bullets and dashes followed by a space, `1.` / `12)` and `a)` at the start of a row.
fn starts_with_list_marker(row: &str) -> bool {
    let mut chars = row.chars();
    let Some(first) = chars.next() else { return false };
    if matches!(first, '•' | '·' | '◦' | '▪' | '‣' | '●' | '■' | '–' | '—' | '-' | '*') {
        let rest = chars.as_str();
        return rest.is_empty() || rest.starts_with(char::is_whitespace);
    }
    let digits = row.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 3 {
        // ASCII digits are one byte each, so `digits` is a char boundary.
        let after = row.get(digits..).unwrap_or("");
        return after.starts_with(')') || after.strip_prefix('.').is_some_and(|t| t.starts_with(char::is_whitespace));
    }
    first.is_ascii_alphabetic() && chars.as_str().starts_with(')')
}

/// `gap` (the text between two spans of `editor_text`, `row` rows after its start) with each
/// newline made a [`BREAK`] when it is hard or `force`d, and a space otherwise.
fn render_gap(gap: &str, mut row: usize, hard: &[bool], force: bool) -> String {
    let mut out = String::with_capacity(gap.len());
    for c in gap.chars() {
        if c == '\n' {
            if force || hard.get(row).copied().unwrap_or(false) {
                out.push_str(BREAK);
            } else {
                out.push(' ');
            }
            row = row.saturating_add(1);
        } else {
            out.push(c);
        }
    }
    out
}

/// `block` with its style runs and hard line breaks marked. A single-style block without a hard
/// break gives `None` (nothing to keep); with hard breaks it gives the text with `⟦br⟧` and no
/// runs. A break between rows is kept (as `⟦br⟧`) where the style changes size, where the next
/// row is a list item, or after a short sentence-ending row; elsewhere it was the old width's
/// wrap and becomes a space.
pub fn tag_runs(doc: &Document, page: usize, block: &TextBlock) -> Result<Option<TaggedBlock>, EditError> {
    let lines = text_lines(doc, page)?;
    let mut runs: Vec<RunStyle> = Vec::new();
    let mut spans: Vec<(Range<usize>, usize)> = Vec::new();
    for (range, index) in &block.spans {
        let Some(line) = lines.get(*index) else { continue };
        let style = RunStyle { resource: line.font.clone(), size: line.size, color: line.color, bold: line.bold, italic: line.italic, family: super::fallback::source_family(&line.base_font) };
        let id = match runs.iter().position(|r| same_look(r, &style)) {
            Some(id) => id,
            None => {
                runs.push(style);
                runs.len() - 1
            }
        };
        spans.push((range.clone(), id));
    }
    let hard = hard_breaks(&block.editor_text);
    if runs.len() < 2 {
        if !hard.iter().any(|h| *h) {
            return Ok(None);
        }
        return Ok(Some(TaggedBlock { text: render_gap(&block.editor_text, 0, &hard, false), runs: Vec::new() }));
    }
    let mut text = String::new();
    let mut open: Option<usize> = None;
    let mut end = 0;
    for (range, id) in &spans {
        let (Some(piece), Some(gap)) = (block.editor_text.get(range.clone()), block.editor_text.get(end..range.start)) else { continue };
        let row = block.editor_text.get(..end).map_or(0, |t| t.matches('\n').count());
        end = range.end;
        let previous = open;
        let gap = render_gap(gap, row, &hard, size_changes(&runs, previous, *id));
        if open != Some(*id) {
            if let Some(n) = open {
                text.push_str(&format!("{OPEN}/{}{CLOSE}", n + 1));
            }
            text.push_str(&gap);
            text.push_str(&format!("{OPEN}{}{CLOSE}", id + 1));
            open = Some(*id);
        } else {
            text.push_str(&gap);
        }
        text.push_str(piece);
    }
    if let Some(n) = open {
        text.push_str(&format!("{OPEN}/{}{CLOSE}", n + 1));
    }
    Ok(Some(TaggedBlock { text, runs }))
}

fn same_look(a: &RunStyle, b: &RunStyle) -> bool {
    a.resource == b.resource
        && (a.size - b.size).abs() < 0.05
        && a.color.iter().zip(&b.color).all(|(x, y)| (x - y).abs() < 0.01)
        && a.bold == b.bold
        && a.italic == b.italic
}

fn size_changes(runs: &[RunStyle], from: Option<usize>, to: usize) -> bool {
    match (from.and_then(|f| runs.get(f)), runs.get(to)) {
        (Some(a), Some(b)) => (a.size - b.size).abs() >= 0.5,
        _ => false,
    }
}

/// What [`untag_runs`] makes of a translated text.
#[derive(Clone, Debug, PartialEq)]
pub struct Untagged {
    /// The text without marks.
    pub text: String,
    /// Each marked stretch drawn in its run's own font resource, size and colour.
    pub exact: BlockStyle,
    /// The same stretches in the standard family closest to the run's font (bold and italic kept),
    /// with its size and colour: for when a subset font lacks a letter of the new text.
    pub standard: BlockStyle,
}

/// Remove the marks from `tagged`, and describe how each marked stretch is drawn. Marks that don't
/// name a run, or that are unbalanced, are dropped and their text stays; text outside any mark
/// keeps the paragraph's own style.
pub fn untag_runs(tagged: &str, runs: &[RunStyle]) -> Untagged {
    let mut text = String::new();
    let mut stretches: Vec<(Range<usize>, usize)> = Vec::new();
    let mut current: Option<(usize, usize)> = None; // run, start byte in `text`
    let mut rest = tagged;
    let flush = |current: &mut Option<(usize, usize)>, text: &str, stretches: &mut Vec<(Range<usize>, usize)>| {
        if let Some((id, start)) = current.take()
            && start < text.len()
        {
            stretches.push((start..text.len(), id));
        }
    };
    while let Some(at) = rest.find(OPEN) {
        let (before, after) = rest.split_at(at);
        text.push_str(before);
        let Some(close) = after.find(CLOSE) else {
            // No closing bracket: not a mark.
            text.push_str(after);
            rest = "";
            break;
        };
        let whole = after.get(..close + CLOSE.len_utf8()).unwrap_or("");
        let inner = after.get(OPEN.len_utf8()..close).unwrap_or("");
        rest = after.get(close + CLOSE.len_utf8()..).unwrap_or("");
        if inner == "br" {
            // A hard line break; a leading or repeated one adds nothing.
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
        } else if let Some(number) = inner.strip_prefix('/') {
            if number.parse::<usize>().is_ok() {
                flush(&mut current, &text, &mut stretches);
            } else {
                text.push_str(whole);
            }
        } else if let Ok(number) = inner.parse::<usize>() {
            flush(&mut current, &text, &mut stretches);
            current = number.checked_sub(1).filter(|id| *id < runs.len()).map(|id| (id, text.len()));
        } else {
            text.push_str(whole);
        }
    }
    text.push_str(rest);
    // A trailing break is dropped; ranges that reached into it are clamped (and dropped if empty).
    text.truncate(text.trim_end_matches('\n').len());
    flush(&mut current, &text, &mut stretches);
    stretches.retain_mut(|(range, _)| {
        range.end = range.end.min(text.len());
        range.start < range.end
    });
    let ranges = |standard: bool| BlockStyle {
        ranges: stretches
            .iter()
            .filter_map(|(range, id)| {
                let run = runs.get(*id)?;
                Some(RangeStyle {
                    range: range.clone(),
                    family: standard.then_some((run.family, run.bold, run.italic)),
                    resource: (!standard).then(|| run.resource.clone()),
                    size: Some(run.size),
                    color: Some(run.color),
                    underline: None,
                    char_spacing: None,
                    scale: None,
                })
            })
            .collect(),
        ..BlockStyle::default()
    };
    Untagged { exact: ranges(false), standard: ranges(true), text }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_items_are_hard_breaks_and_wrapped_prose_is_not() {
        assert_eq!(hard_breaks("• Problem\n• Prioritize\n• Measure"), [true, true]);
        assert_eq!(hard_breaks("1. One\n2) Two\na) Three"), [true, true]);
        assert_eq!(hard_breaks("a long row of running text\nthat wraps on and on\nuntil it ends"), [false, false]);
        // No space after the dash, a decimal and a word in brackets are not markers.
        assert_eq!(hard_breaks("text here and there\n-5 degrees\n1.5 metres\nab) no"), [false, false, false]);
    }

    #[test]
    fn a_short_sentence_end_breaks_only_when_the_row_is_short() {
        assert_eq!(hard_breaks("A very long row of text ends here.\nShort.\nnext"), [false, true]);
        assert_eq!(hard_breaks("A very long row of text ends here\nShort\nnext row"), [false, false]);
        assert_eq!(hard_breaks(""), Vec::<bool>::new());
        assert_eq!(hard_breaks("日本語。\n\n"), [false, false]);
    }

    #[test]
    fn gaps_render_breaks_spaces_and_survive_out_of_range_rows() {
        assert_eq!(render_gap("\n", 0, &[true], false), "⟦br⟧");
        assert_eq!(render_gap("\n", 0, &[false], false), " ");
        assert_eq!(render_gap("\n", 0, &[false], true), "⟦br⟧");
        assert_eq!(render_gap(" \n\n é", 5, &[true], false), "    é");
    }
}
