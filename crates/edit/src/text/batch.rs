//! Rewriting many paragraphs of one page, each found again by its key just before it is edited.
//!
//! The index of a paragraph is its position in a list regrouped from geometry on every read, so
//! rewriting one paragraph can merge or split the others (#2516). Replacing by index, one after
//! another, then edits the wrong paragraph or deletes a neighbour. Here every edit names its
//! paragraph by [`BlockKey`], is tried on a copy, and is kept only when every paragraph still to be
//! edited can still be found afterwards. An edit that would merge paragraphs waits until the others
//! are done; one that still can't be applied is reported, never forced.

use pdfcraft_cos::Document;

use super::{BlockKey, BlockStyle, EditError, TextBlock, block_overlapping, rewrite_block, text_blocks};

/// One paragraph to rewrite: the key recorded when the page was listed, and its new text.
#[derive(Clone, Debug)]
pub struct BlockEdit {
    pub key: BlockKey,
    pub text: String,
    /// Formatting for the new text: style ranges from [`super::untag_runs`], for instance.
    pub style: BlockStyle,
    /// Tried when the engine can't apply `style` (a font lacking a letter); after it, and last,
    /// the paragraph's own style.
    pub fallback: Option<BlockStyle>,
}

impl BlockEdit {
    pub fn new(key: BlockKey, text: impl Into<String>) -> Self {
        Self { key, text: text.into(), style: BlockStyle::default(), fallback: None }
    }
}

/// What happened to one [`BlockEdit`].
#[derive(Clone, Debug, PartialEq)]
pub enum BlockOutcome {
    /// Rewritten. `scale` is the font size relative to the original (1.0: unchanged); `overflow`
    /// is set when the text still doesn't fit its original box at the smallest size allowed.
    /// `plain` is set when the styles asked for couldn't be applied and the paragraph's own was used.
    Applied { substituted: Option<String>, scale: f64, overflow: bool, plain: bool },
    /// The new text is empty or the same as the old one: the paragraph was left as it was.
    Unchanged,
    /// No paragraph on the page has this key any more (or never had).
    Missing,
    /// Rewriting it would merge it with, or lose, another paragraph still to be edited.
    Merges,
    /// The rewrite itself was refused.
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BatchReport {
    /// One outcome per edit, in the order of the edits given.
    pub outcomes: Vec<BlockOutcome>,
}

/// Sizes tried, from the original down, until the new text fits the paragraph's box.
const SCALES: [f64; 7] = [1.0, 0.95, 0.9, 0.85, 0.8, 0.75, 0.7];
/// A paragraph may end this many points below where it ended (rounding in the layout).
const SLACK: f64 = 1.0;

/// Rewrite `edits` on `page` (0-based). Paragraphs that can't be rewritten safely are reported in
/// the result and left untouched. `min_scale` (0.0–1.0) is the smallest font size, relative to the
/// original, tried to make longer text fit.
pub fn rewrite_page_blocks(doc: &mut Document, page: usize, edits: &[BlockEdit], min_scale: f64) -> Result<BatchReport, EditError> {
    let mut outcomes = vec![BlockOutcome::Missing; edits.len()];
    let mut pending: Vec<usize> = (0..edits.len()).collect();
    while !pending.is_empty() {
        let mut deferred = Vec::new();
        let mut progressed = false;
        for (n, &i) in pending.iter().enumerate() {
            let Some(edit) = edits.get(i) else { continue };
            // Everything still to be edited, but this one: it has to survive this edit.
            let others: Vec<&BlockKey> =
                pending.iter().skip(n + 1).chain(deferred.iter()).filter_map(|&j| edits.get(j)).map(|e| &e.key).collect();
            match try_edit(doc, page, edit, &others, min_scale)? {
                Attempt::Done(outcome) => {
                    progressed |= matches!(outcome, BlockOutcome::Applied { .. });
                    set(&mut outcomes, i, outcome);
                }
                Attempt::Merges => deferred.push(i),
            }
        }
        if !progressed {
            // Nothing changed in a whole pass, so waiting longer won't help.
            for &i in &deferred {
                set(&mut outcomes, i, BlockOutcome::Merges);
            }
            break;
        }
        pending = deferred;
    }
    Ok(BatchReport { outcomes })
}

enum Attempt {
    Done(BlockOutcome),
    Merges,
}

fn set(outcomes: &mut [BlockOutcome], i: usize, outcome: BlockOutcome) {
    if let Some(slot) = outcomes.get_mut(i) {
        *slot = outcome;
    }
}

fn try_edit(doc: &mut Document, page: usize, edit: &BlockEdit, others: &[&BlockKey], min_scale: f64) -> Result<Attempt, EditError> {
    let blocks = text_blocks(doc, page)?;
    let Some(at) = edit.key.find(&blocks, 0) else { return Ok(Attempt::Done(BlockOutcome::Missing)) };
    let Some(block) = blocks.get(at) else { return Ok(Attempt::Done(BlockOutcome::Missing)) };
    if edit.text.trim().is_empty() || edit.text == block.text || edit.text == block.reflow_text {
        return Ok(Attempt::Done(BlockOutcome::Unchanged));
    }
    // NaN compares false everywhere, so it falls through to the original size only.
    let floor = if min_scale.is_finite() { min_scale.clamp(0.1, 1.0) } else { 1.0 };
    let mut merged = false;
    let mut last: Option<(Document, Option<String>, f64, bool)> = None;
    for &scale in SCALES.iter().filter(|s| **s >= floor || **s == 1.0) {
        let sized = |style: &BlockStyle| {
            let mut style = style.clone();
            if scale < 1.0 {
                style.size = Some(block.size * scale);
                for range in &mut style.ranges {
                    range.size = range.size.map(|s| s * scale);
                }
            }
            style
        };
        // The styles asked for, then their stand-in, then the paragraph's own: each only when the
        // one before was refused.
        let mut tiers: Vec<&BlockStyle> = Vec::new();
        if !edit.style.ranges.is_empty() {
            tiers.push(&edit.style);
            tiers.extend(edit.fallback.as_ref());
        }
        let own = BlockStyle::default();
        tiers.push(&own);
        let mut plain = false;
        let (mut trial, mut result) = (doc.clone(), Err(EditError::Invalid("no style could be tried".into())));
        for (n, style) in tiers.iter().enumerate() {
            trial = doc.clone();
            result = rewrite_block(&mut trial, page, at, Some(&edit.text), &sized(style));
            plain = n > 0;
            if result.is_ok() {
                break;
            }
        }
        let substituted = match result {
            Ok(done) => done.substituted,
            Err(e) => return Ok(Attempt::Done(BlockOutcome::Failed(e.to_string()))),
        };
        let after = text_blocks(&trial, page)?;
        if !others.iter().all(|key| key.find(&after, 0).is_some()) {
            merged = true;
            break;
        }
        if fits(block, &after) {
            *doc = trial;
            return Ok(Attempt::Done(BlockOutcome::Applied { substituted, scale, overflow: false, plain }));
        }
        last = Some((trial, substituted, scale, plain));
    }
    if merged {
        return Ok(Attempt::Merges);
    }
    // It never fit: keep the smallest size tried, and say so.
    match last {
        Some((trial, substituted, scale, plain)) => {
            *doc = trial;
            Ok(Attempt::Done(BlockOutcome::Applied { substituted, scale, overflow: true, plain }))
        }
        None => Ok(Attempt::Done(BlockOutcome::Failed("no size could be tried".into()))),
    }
}

/// Whether the paragraph `original` became, in `after`, still ends where it did.
fn fits(original: &TextBlock, after: &[TextBlock]) -> bool {
    let Some(now) = block_overlapping(after, original.rect).and_then(|i| after.get(i)) else { return false };
    now.rect[1] >= original.rect[1] - SLACK
}
