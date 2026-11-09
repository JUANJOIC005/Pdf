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

/// `block` with its style runs marked, or `None` when it has a single style (nothing to keep).
/// A line break between rows is kept only where the style changes size, a heading over its body;
/// elsewhere it was the old width's wrap, and becomes a space.
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
    if runs.len() < 2 {
        return Ok(None);
    }
    let mut text = String::new();
    let mut open: Option<usize> = None;
    let mut end = 0;
    for (range, id) in &spans {
        let (Some(piece), Some(gap)) = (block.editor_text.get(range.clone()), block.editor_text.get(end..range.start)) else { continue };
        end = range.end;
        let previous = open;
        let gap = if gap.contains('\n') && !size_changes(&runs, previous, *id) { gap.replace('\n', " ") } else { gap.to_string() };
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
        if let Some(number) = inner.strip_prefix('/') {
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
    flush(&mut current, &text, &mut stretches);
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
