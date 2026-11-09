//! Markdown for the AI chat panel: [`parse`] (no egui, bounded, tolerant of half-streamed text),
//! [`show`] (egui painting in the app tokens) and [`MarkdownCache`].

mod highlight;
mod parse;
mod render;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub use parse::{Align, Block, Inline, MAX_BLOCKS, MAX_DEPTH, MAX_INPUT, parse};
pub use render::{math_text, show};

/// Keeps the last parse so an unchanged message is not parsed again every frame.
///
/// Parsing is linear in the text, but a streamed answer changes on every chunk: the caller should
/// limit how often it asks (for example, once per frame at most, or every few chunks), because
/// each changed text is parsed in full again.
#[derive(Default)]
pub struct MarkdownCache {
    text_len: usize,
    hash: u64,
    blocks: Arc<Vec<Block>>,
}

impl MarkdownCache {
    /// The parsed blocks of `text`, re-parsed only if its length or content changed.
    pub fn get(&mut self, text: &str) -> Arc<Vec<Block>> {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        let hash = h.finish();
        if self.text_len != text.len() || self.hash != hash {
            self.blocks = Arc::new(parse(text));
            self.text_len = text.len();
            self.hash = hash;
        }
        Arc::clone(&self.blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reuses_unchanged_text() {
        let mut c = MarkdownCache::default();
        let a = c.get("# hi");
        let b = c.get("# hi");
        assert!(Arc::ptr_eq(&a, &b));
        let d = c.get("# ho");
        assert!(!Arc::ptr_eq(&a, &d));
        assert!(c.get("").is_empty());
    }
}
