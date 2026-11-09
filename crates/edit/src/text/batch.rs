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
    /// The engine rewrote it but dropped characters at every style and size tried (a glyph the
    /// font lacks), or the new text has no letters where the old one had. Nothing was applied;
    /// `missing` holds the characters that would have been lost.
    Lost { missing: String },
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
            let others: Vec<&BlockKey> = pending.iter().skip(n + 1).chain(deferred.iter()).filter_map(|&j| edits.get(j)).map(|e| &e.key).collect();
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
    // Nothing alphabetic in the new text while the old one had words: they would vanish. A number
    // changed for another ("2024" to "2025", a localised "1.234,56") is a legitimate edit.
    if !is_translatable(&edit.text) && is_translatable(&block.text) {
        return Ok(Attempt::Done(BlockOutcome::Lost { missing: block.text.chars().filter(|c| c.is_alphabetic()).collect() }));
    }
    // The visible page area; without a readable page there is no right edge to hold to.
    let crop = pdfcraft_model::pages(doc).get(page).map(|p| p.crop(doc));
    // NaN compares false everywhere, so it falls through to the original size only.
    let floor = if min_scale.is_finite() { min_scale.clamp(0.1, 1.0) } else { 1.0 };
    let mut merged = false;
    let mut lost: Option<String> = None;
    let mut last: Option<(Document, Option<String>, f64, bool)> = None;
    'scales: for &scale in SCALES.iter().filter(|s| **s >= floor || **s == 1.0) {
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
        let mut refusal: Option<String> = None;
        let mut any_applied = false;
        for (n, style) in tiers.iter().enumerate() {
            let mut trial = doc.clone();
            let substituted = match rewrite_block(&mut trial, page, at, Some(&edit.text), &sized(style)) {
                Ok(done) => done.substituted,
                Err(e) => {
                    refusal = Some(e.to_string());
                    continue;
                }
            };
            any_applied = true;
            let after = text_blocks(&trial, page)?;
            if !others.iter().all(|key| key.find(&after, 0).is_some()) {
                merged = true;
                break 'scales;
            }
            let missing = missing_in(&edit.text, &after, block.rect);
            if !missing.is_empty() {
                // The engine said yes but dropped characters: try the next style.
                lost.get_or_insert(missing);
                continue;
            }
            if fits(block, &blocks, &after, crop) {
                *doc = trial;
                return Ok(Attempt::Done(BlockOutcome::Applied { substituted, scale, overflow: false, plain: n > 0 }));
            }
            last = Some((trial, substituted, scale, n > 0));
            break;
        }
        if !any_applied {
            return Ok(Attempt::Done(BlockOutcome::Failed(refusal.unwrap_or_else(|| "no style could be tried".into()))));
        }
    }
    if merged {
        return Ok(Attempt::Merges);
    }
    // It never fit: keep the smallest size tried that kept all the text, and say so.
    match (last, lost) {
        (Some((trial, substituted, scale, plain)), _) => {
            *doc = trial;
            Ok(Attempt::Done(BlockOutcome::Applied { substituted, scale, overflow: true, plain }))
        }
        // Every attempt dropped some of the text: the paragraph stays as it was.
        (None, Some(missing)) => Ok(Attempt::Done(BlockOutcome::Lost { missing })),
        (None, None) => Ok(Attempt::Done(BlockOutcome::Failed("no size could be tried".into()))),
    }
}

/// Whether the paragraph `original` became, in `after`, still ends where it did, stays inside the
/// page's right edge and doesn't cover another paragraph it didn't already overlap.
fn fits(original: &TextBlock, before: &[TextBlock], after: &[TextBlock], crop: Option<[f64; 4]>) -> bool {
    let Some(at) = block_overlapping(after, original.rect) else { return false };
    let Some(now) = after.get(at) else { return false };
    if now.rect[1] < original.rect[1] - SLACK {
        return false;
    }
    // A paragraph that already ran past the edge may keep its extent.
    if let Some(crop) = crop
        && now.rect[2] > crop[2].max(original.rect[2]) + SLACK
    {
        return false;
    }
    let overlaps = |a: &[f64; 4], b: &[f64; 4]| {
        a.iter().chain(b).all(|v| v.is_finite()) && a[2].min(b[2]) - a[0].max(b[0]) > SLACK && a[3].min(b[3]) - a[1].max(b[1]) > SLACK
    };
    // Overlaps that existed before the edit don't count.
    let already = |rect: &[f64; 4]| overlaps(&original.rect, rect);
    // A neighbour the new text swallowed into its own box shows as one box, not as an overlap.
    let swallowed = before.iter().any(|b| b.rect != original.rect && !already(&b.rect) && block_overlapping(after, b.rect) == Some(at));
    !swallowed
        && after
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != at)
            .all(|(_, other)| !overlaps(&now.rect, &other.rect) || before.iter().any(|b| b.rect == other.rect && already(&b.rect)))
}

/// Whether `text` has anything to translate: at least one alphabetic letter. Numbers, signs,
/// symbols and spaces alone ("4", "© 2025", "→") have none.
pub fn is_translatable(text: &str) -> bool {
    text.chars().any(char::is_alphabetic)
}

/// The characters of `requested` that the paragraph overlapping `rect` in `after` doesn't show.
fn missing_in(requested: &str, after: &[TextBlock], rect: [f64; 4]) -> String {
    let now = block_overlapping(after, rect).and_then(|i| after.get(i)).map_or("", |b| b.text.as_str());
    missing_chars(requested, now)
}

/// The characters of `requested` absent from `now`, compared without spaces, line breaks and
/// hyphens (a wrap may join or drop them) and with quotes, ligatures and bullets folded.
fn missing_chars(requested: &str, now: &str) -> String {
    let mut have: std::collections::HashMap<char, usize> = std::collections::HashMap::new();
    for c in now.chars().flat_map(fold) {
        *have.entry(c).or_insert(0) += 1;
    }
    let mut out = String::new();
    for c in requested.chars().flat_map(fold) {
        match have.get_mut(&c) {
            Some(n) if *n > 0 => *n -= 1,
            _ => out.push(c),
        }
    }
    out
}

/// One character as [`missing_chars`] compares it: nothing for what a layout may add or drop.
fn fold(c: char) -> Vec<char> {
    match c {
        c if c.is_whitespace() => vec![],
        '-' | '\u{2010}'..='\u{2015}' | '\u{2212}' | '\u{00AD}' | '\u{200B}'..='\u{200D}' | '\u{FEFF}' => vec![],
        '\u{2018}' | '\u{2019}' | '\u{201B}' => vec!['\''],
        '\u{201C}' | '\u{201D}' | '\u{201F}' => vec!['"'],
        '\u{FB00}' => vec!['f', 'f'],
        '\u{FB01}' => vec!['f', 'i'],
        '\u{FB02}' => vec!['f', 'l'],
        '\u{FB03}' => vec!['f', 'f', 'i'],
        '\u{FB04}' => vec!['f', 'f', 'l'],
        '\u{2022}' | '\u{00B7}' | '\u{2219}' | '\u{25CF}' | '\u{25AA}' => vec!['\u{2022}'],
        c => vec![c],
    }
}

#[cfg(test)]
mod tests {
    use super::missing_chars;

    /// What the engine reads back after it dropped a glyph (an arrow, the space after a bullet) is
    /// reported; what only a wrap or a font changes is not.
    #[test]
    fn missing_chars_reports_dropped_glyphs_and_ignores_layout_noise() {
        assert_eq!(missing_chars("Next \u{2192} step", "Next  step"), "\u{2192}");
        assert_eq!(missing_chars("\u{2022} Item", "Item"), "\u{2022}");
        assert_eq!(missing_chars("aab", "ab"), "a");
        assert_eq!(missing_chars("Next step", "Next\nstep"), "");
        assert_eq!(missing_chars("\u{2022} Item", "\u{00B7}Item"), "");
        assert_eq!(missing_chars("\u{201C}hi\u{201D} \u{FB01}ne", "\"hi\" fine"), "");
        assert_eq!(missing_chars("co-operate", "cooperate"), "");
        assert_eq!(missing_chars("", "anything"), "");
        assert_eq!(missing_chars("x", ""), "x");
    }
}
