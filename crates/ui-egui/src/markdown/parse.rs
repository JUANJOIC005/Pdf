//! Markdown to a small block tree, with no egui. The input is untrusted model output that may be
//! cut anywhere (streaming), so the parser is total: it never panics, truncates oversized input,
//! flattens nesting past [`MAX_DEPTH`] to text and stops after [`MAX_BLOCKS`] blocks. Raw HTML is
//! kept as plain text and never interpreted.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Longest input parsed, in bytes; the rest is dropped at a character boundary.
pub const MAX_INPUT: usize = 1 << 20;
/// Deepest container nesting kept; deeper content is flattened into the enclosing text.
pub const MAX_DEPTH: usize = 32;
/// Most blocks (counted across all nesting levels) kept.
pub const MAX_BLOCKS: usize = 2000;

/// Column alignment of a table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    None,
    Left,
    Center,
    Right,
}

/// Inline content.
#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String),
    Strong(Vec<Inline>),
    Emphasis(Vec<Inline>),
    Strike(Vec<Inline>),
    Code(String),
    /// A link or an image (shown as its alt text); the url is unchecked, the renderer filters it.
    Link { text: Vec<Inline>, url: String },
    /// A hard line break.
    Break,
    /// A `$…$` formula, source text as written.
    Math(String),
}

/// Block content.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Heading { level: u8, inlines: Vec<Inline> },
    Paragraph(Vec<Inline>),
    /// `checked` has one entry per item: `Some` for a task-list item.
    List { ordered: bool, start: u64, items: Vec<Vec<Block>>, checked: Vec<Option<bool>> },
    Quote(Vec<Block>),
    Code { lang: Option<String>, text: String },
    Table { align: Vec<Align>, head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>> },
    Rule,
}

#[derive(Clone, Copy)]
enum Kind {
    Root,
    Quote,
    Item(Option<bool>),
}

enum Frame {
    /// A container of blocks; loose inlines (tight list items) wait in `pending`.
    Container { kind: Kind, blocks: Vec<Block>, pending: Vec<Inline> },
    List { ordered: bool, start: u64, items: Vec<Vec<Block>>, checked: Vec<Option<bool>>, stray: Vec<Inline> },
    Heading { level: u8, inl: Vec<Inline> },
    Paragraph(Vec<Inline>),
    Code { lang: Option<String>, text: String },
    Table { align: Vec<Align>, head: Vec<Vec<Inline>>, rows: Vec<Vec<Vec<Inline>>>, row: Vec<Vec<Inline>> },
    Cell(Vec<Inline>),
    Strong(Vec<Inline>),
    Emphasis(Vec<Inline>),
    Strike(Vec<Inline>),
    Link { url: String, inl: Vec<Inline> },
}

struct Builder {
    stack: Vec<Frame>,
    /// Start tags ignored for being past the depth limit, so their ends are ignored too.
    overflow: usize,
    count: usize,
}

fn take_para(pending: &mut Vec<Inline>) -> Option<Block> {
    if pending.is_empty() { None } else { Some(Block::Paragraph(std::mem::take(pending))) }
}

impl Builder {
    fn full(&self) -> bool {
        self.count >= MAX_BLOCKS
    }

    /// Where inline content goes now, if anywhere.
    fn sink(&mut self) -> Option<&mut Vec<Inline>> {
        match self.stack.last_mut()? {
            Frame::Container { pending, .. } => Some(pending),
            Frame::List { stray, .. } => Some(stray),
            Frame::Heading { inl, .. } | Frame::Link { inl, .. } => Some(inl),
            Frame::Paragraph(v) | Frame::Cell(v) | Frame::Strong(v) | Frame::Emphasis(v) | Frame::Strike(v) => Some(v),
            Frame::Code { .. } | Frame::Table { .. } => None,
        }
    }

    fn inline(&mut self, i: Inline) {
        if let Some(s) = self.sink() {
            s.push(i);
        }
    }

    fn text(&mut self, t: &str) {
        if let Some(Frame::Code { text, .. }) = self.stack.last_mut() {
            text.push_str(t);
            return;
        }
        if let Some(s) = self.sink() {
            if let Some(Inline::Text(last)) = s.last_mut() {
                last.push_str(t);
            } else {
                s.push(Inline::Text(t.to_owned()));
            }
        }
    }

    fn push_block(&mut self, b: Block) {
        if self.full() {
            return;
        }
        self.count += 1;
        if let Some(Frame::Container { blocks, pending, .. }) = self.stack.last_mut() {
            blocks.extend(take_para(pending));
            blocks.push(b);
        }
    }

    fn push_frame(&mut self, f: Frame) {
        if self.stack.len() >= MAX_DEPTH {
            self.overflow += 1;
        } else {
            self.stack.push(f);
        }
    }

    fn container(kind: Kind) -> Frame {
        Frame::Container { kind, blocks: Vec::new(), pending: Vec::new() }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.push_frame(Frame::Paragraph(Vec::new())),
            Tag::Heading { level, .. } => self.push_frame(Frame::Heading { level: heading_level(level), inl: Vec::new() }),
            Tag::BlockQuote(_) => self.push_frame(Self::container(Kind::Quote)),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => info.split_whitespace().next().map(|s| s.chars().take(32).collect()),
                    CodeBlockKind::Indented => None,
                };
                self.push_frame(Frame::Code { lang, text: String::new() });
            }
            Tag::List(start) => self.push_frame(Frame::List {
                ordered: start.is_some(),
                start: start.unwrap_or(1),
                items: Vec::new(),
                checked: Vec::new(),
                stray: Vec::new(),
            }),
            Tag::Item => self.push_frame(Self::container(Kind::Item(None))),
            Tag::Table(al) => self.push_frame(Frame::Table {
                align: al.iter().map(|a| align(*a)).collect(),
                head: Vec::new(),
                rows: Vec::new(),
                row: Vec::new(),
            }),
            Tag::TableHead | Tag::TableRow => {
                // Not a frame of its own: the cells collect into the table's current row.
                if let Some(Frame::Table { row, .. }) = self.stack.last_mut() {
                    row.clear();
                }
            }
            Tag::TableCell => self.push_frame(Frame::Cell(Vec::new())),
            Tag::Emphasis => self.push_frame(Frame::Emphasis(Vec::new())),
            Tag::Strong => self.push_frame(Frame::Strong(Vec::new())),
            Tag::Strikethrough => self.push_frame(Frame::Strike(Vec::new())),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                self.push_frame(Frame::Link { url: dest_url.into_string(), inl: Vec::new() });
            }
            // HTML blocks, footnote definitions and the like: their text flows into the parent.
            _ => self.overflow += 1,
        }
    }

    fn end(&mut self, tag: TagEnd) {
        if matches!(tag, TagEnd::TableHead | TagEnd::TableRow) {
            if let Some(Frame::Table { head, rows, row, .. }) = self.stack.last_mut() {
                let done = std::mem::take(row);
                if matches!(tag, TagEnd::TableHead) {
                    *head = done;
                } else if rows.len() < MAX_BLOCKS {
                    rows.push(done);
                }
            }
            return;
        }
        if self.overflow > 0 {
            self.overflow -= 1;
            return;
        }
        self.pop();
    }

    /// Closes the top frame into its parent.
    fn pop(&mut self) {
        if self.stack.len() <= 1 {
            return;
        }
        let Some(frame) = self.stack.pop() else { return };
        match frame {
            Frame::Paragraph(v) => self.push_block(Block::Paragraph(v)),
            Frame::Heading { level, inl } => self.push_block(Block::Heading { level, inlines: inl }),
            Frame::Code { lang, text } => self.push_block(Block::Code { lang, text }),
            Frame::Container { kind, mut blocks, mut pending } => {
                blocks.extend(take_para(&mut pending));
                match kind {
                    Kind::Quote => self.push_block(Block::Quote(blocks)),
                    Kind::Item(c) => {
                        if let Some(Frame::List { items, checked, .. }) = self.stack.last_mut() {
                            items.push(blocks);
                            checked.push(c);
                        }
                    }
                    Kind::Root => {}
                }
            }
            Frame::List { ordered, start, mut items, mut checked, stray } => {
                if !stray.is_empty() {
                    items.push(vec![Block::Paragraph(stray)]);
                    checked.push(None);
                }
                self.push_block(Block::List { ordered, start, items, checked });
            }
            Frame::Table { align, head, rows, .. } => self.push_block(Block::Table { align, head, rows }),
            Frame::Cell(v) => {
                if let Some(Frame::Table { row, .. }) = self.stack.last_mut() {
                    row.push(v);
                }
            }
            Frame::Strong(v) => self.inline(Inline::Strong(v)),
            Frame::Emphasis(v) => self.inline(Inline::Emphasis(v)),
            Frame::Strike(v) => self.inline(Inline::Strike(v)),
            Frame::Link { url, inl } => self.inline(Inline::Link { text: inl, url }),
        }
    }

    fn event(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(t) => self.start(t),
            Event::End(t) => self.end(t),
            Event::Text(t) | Event::Html(t) | Event::InlineHtml(t) => self.text(&t),
            Event::Code(c) => self.inline(Inline::Code(c.into_string())),
            Event::InlineMath(m) | Event::DisplayMath(m) => self.inline(Inline::Math(m.into_string())),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.inline(Inline::Break),
            Event::Rule => self.push_block(Block::Rule),
            Event::FootnoteReference(t) => self.text(&format!("[{t}]")),
            Event::TaskListMarker(c) => {
                if let Some(Frame::Container { kind: Kind::Item(slot), .. }) = self.stack.last_mut() {
                    *slot = Some(c);
                }
            }
        }
    }

    fn finish(mut self) -> Vec<Block> {
        while self.stack.len() > 1 {
            self.pop();
        }
        match self.stack.pop() {
            Some(Frame::Container { mut blocks, mut pending, .. }) => {
                blocks.extend(take_para(&mut pending));
                blocks
            }
            _ => Vec::new(),
        }
    }
}

fn heading_level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn align(a: Alignment) -> Align {
    match a {
        Alignment::None => Align::None,
        Alignment::Left => Align::Left,
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
    }
}

/// Parses Markdown (CommonMark plus tables, strikethrough, task lists and `$…$` math) into blocks.
/// Incomplete input is fine: open fences, tables and emphasis are closed or shown as text.
pub fn parse(src: &str) -> Vec<Block> {
    let mut end = src.len().min(MAX_INPUT);
    while !src.is_char_boundary(end) {
        end -= 1; // terminates: index 0 is always a boundary
    }
    let src = src.get(..end).unwrap_or("");
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_MATH;
    let mut b = Builder { stack: vec![Builder::container(Kind::Root)], overflow: 0, count: 0 };
    for ev in Parser::new_ext(src, opts) {
        b.event(ev);
        if b.full() {
            break;
        }
    }
    b.finish()
}

/// Plain text of inlines (alt text, accessibility, copy).
pub fn plain(inl: &[Inline]) -> String {
    let mut s = String::new();
    for i in inl {
        match i {
            Inline::Text(t) | Inline::Code(t) | Inline::Math(t) => s.push_str(t),
            Inline::Strong(v) | Inline::Emphasis(v) | Inline::Strike(v) | Inline::Link { text: v, .. } => s.push_str(&plain(v)),
            Inline::Break => s.push('\n'),
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Inline {
        Inline::Text(s.into())
    }

    #[test]
    fn empty_input() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn paragraph_with_marks_and_link() {
        let b = parse("a **b** *c* ~~d~~ `e` [x](https://x.org)");
        let Some(Block::Paragraph(p)) = b.first() else { panic!("{b:?}") };
        assert_eq!(p[1], Inline::Strong(vec![t("b")]));
        assert_eq!(p[3], Inline::Emphasis(vec![t("c")]));
        assert_eq!(p[5], Inline::Strike(vec![t("d")]));
        assert_eq!(p[7], Inline::Code("e".into()));
        assert_eq!(p[9], Inline::Link { text: vec![t("x")], url: "https://x.org".into() });
    }

    #[test]
    fn table_with_alignment() {
        let b = parse("| a | b | c |\n|:--|:-:|--:|\n| 1 | 2 | 3 |\n| 4 | 5 | 6 |\n");
        let Some(Block::Table { align, head, rows }) = b.first() else { panic!("{b:?}") };
        assert_eq!(align, &[Align::Left, Align::Center, Align::Right]);
        assert_eq!(head.len(), 3);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1][2], vec![t("6")]);
    }

    #[test]
    fn half_table_is_text() {
        assert!(matches!(parse("| a | b |\n").first(), Some(Block::Paragraph(_))));
        assert!(matches!(parse("| a | b |\n|---|---|").first(), Some(Block::Table { .. })));
    }

    #[test]
    fn nested_and_ordered_lists() {
        let b = parse("3. one\n   - sub\n   - [x] done\n4. two\n");
        let Some(Block::List { ordered, start, items, .. }) = b.first() else { panic!("{b:?}") };
        assert!(*ordered);
        assert_eq!(*start, 3);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0][0], Block::Paragraph(vec![t("one")]));
        let Some(Block::List { ordered, checked, .. }) = items[0].get(1) else { panic!("{items:?}") };
        assert!(!ordered);
        assert_eq!(checked, &[None, Some(true)]);
    }

    #[test]
    fn quote() {
        let b = parse("> hi\n> there\n");
        assert_eq!(b, vec![Block::Quote(vec![Block::Paragraph(vec![t("hi there")])])]);
    }

    #[test]
    fn unclosed_fence_keeps_text() {
        let b = parse("```rust\nfn main() {\n");
        assert_eq!(b, vec![Block::Code { lang: Some("rust".into()), text: "fn main() {\n".into() }]);
    }

    #[test]
    fn open_emphasis_is_text() {
        let Some(Block::Paragraph(p)) = parse("hello **bold").into_iter().next() else { panic!() };
        assert_eq!(plain(&p), "hello **bold");
    }

    #[test]
    fn raw_html_is_text() {
        let b = parse("<script>alert(1)</script>\n\nx <b>y</b>");
        let all: String = b.iter().map(|b| if let Block::Paragraph(p) = b { plain(p) } else { String::new() }).collect();
        assert!(all.contains("<script>"));
        assert!(all.contains("<b>y</b>"));
    }

    #[test]
    fn math_inline() {
        let Some(Block::Paragraph(p)) = parse("see $x^2$ ok").into_iter().next() else { panic!() };
        assert!(p.contains(&Inline::Math("x^2".into())));
    }

    #[test]
    fn rule_and_heading() {
        let b = parse("## T\n\n---\n");
        assert_eq!(b, vec![Block::Heading { level: 2, inlines: vec![t("T")] }, Block::Rule]);
    }

    #[test]
    fn huge_input_is_cut() {
        let big = "palabra ".repeat(1 << 18);
        assert!(big.len() >= 2 << 20);
        let b = parse(&big);
        assert!(b.len() <= MAX_BLOCKS);
        let Some(Block::Paragraph(p)) = b.first() else { panic!() };
        assert!(plain(p).len() <= MAX_INPUT);
    }

    #[test]
    fn cut_inside_multibyte_char() {
        // The limit falls in the middle of a 4-byte char.
        let mut s = "a".repeat(MAX_INPUT - 2);
        s.push_str("😀😀");
        assert!(!s.is_char_boundary(MAX_INPUT));
        assert_eq!(parse(&s).len(), 1);
        assert_eq!(parse(&"é".repeat(MAX_INPUT)).len(), 1);
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let quotes = format!("{}deep", "> ".repeat(100));
        let b = parse(&quotes);
        assert!(depth(&b) <= MAX_DEPTH);
        assert!(text_of(&b).contains("deep"));
        let lists: String = (0..100).map(|i| format!("{}- x{i}\n", "  ".repeat(i))).collect();
        assert!(depth(&parse(&lists)) <= MAX_DEPTH);
        let emph = format!("{}x{}", "*a ".repeat(100), "*".repeat(100));
        let _ = parse(&emph);
    }

    fn depth(bs: &[Block]) -> usize {
        bs.iter()
            .map(|b| match b {
                Block::Quote(v) => 1 + depth(v),
                Block::List { items, .. } => 1 + items.iter().map(|i| depth(i)).max().unwrap_or(0),
                _ => 0,
            })
            .max()
            .unwrap_or(0)
    }

    fn text_of(bs: &[Block]) -> String {
        bs.iter()
            .map(|b| match b {
                Block::Quote(v) => text_of(v),
                Block::Paragraph(p) => plain(p),
                Block::List { items, .. } => items.iter().map(|i| text_of(i)).collect(),
                _ => String::new(),
            })
            .collect()
    }

    #[test]
    fn block_limit() {
        let s = "para\n\n".repeat(5000);
        assert_eq!(parse(&s).len(), MAX_BLOCKS);
        let s = "- a\n\n".repeat(5000);
        assert!(parse(&s).len() <= MAX_BLOCKS);
    }

    #[test]
    fn every_prefix_parses() {
        let doc = "# Título ñ\n\n| a | b |\n|---|:-:|\n| `x` | **y** |\n\n- [ ] uno\n  1. dos\n\n> cita $\\pi$\n\n```py\nprint('é😀')\n```\n[l](http://a)\n";
        for (i, _) in doc.char_indices() {
            let _ = parse(doc.get(..i).unwrap_or(""));
        }
    }
}
