//! The model every converter builds, and its Markdown serializer.
//!
//! A converter parses its input into a [`Document`]. [`Document::write_markdown`]
//! is the only place that decides escaping, block separation and table layout.
//! Tables are laid out by `mq-markdown`.

use std::io::Write;

use mq_markdown::{Html, Markdown, Node, TableAlign, TableAlignKind, TableCell, Text};

use crate::error::Result;

/// A sequence of blocks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    pub blocks: Vec<Block>,
}

/// A block-level element.
#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    /// A heading of level 1 to 6.
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    Paragraph(Vec<Inline>),
    /// Each item is a sequence of blocks; a nested list is a block of its item.
    List {
        ordered: bool,
        items: Vec<Vec<Block>>,
    },
    /// Preformatted text, with an optional info string such as the language.
    Code {
        lang: Option<String>,
        text: String,
    },
    Quote(Vec<Block>),
    Rule,
    /// Rows of cells; the first row is the header.
    Table(Vec<Vec<Vec<Inline>>>),
}

/// An inline element.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
    Strong(Vec<Inline>),
    Strike(Vec<Inline>),
    Code(String),
    Link { url: String, content: Vec<Inline> },
    Image { url: String, alt: String },
}

/// One line of a flat list, as most markup formats write them.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatItem {
    /// Nesting depth, 0 for the outermost list.
    pub depth: usize,
    pub ordered: bool,
    pub content: Vec<Inline>,
}

impl Block {
    /// A heading with plain text.
    pub fn heading(level: u8, text: impl Into<String>) -> Self {
        Self::Heading {
            level,
            content: vec![Inline::Text(text.into())],
        }
    }

    /// A paragraph with plain text.
    pub fn paragraph(text: impl Into<String>) -> Self {
        Self::Paragraph(vec![Inline::Text(text.into())])
    }

    /// A paragraph that is one emphasized run, e.g. `*Empty sheet*`.
    pub fn emphasized(text: impl Into<String>) -> Self {
        Self::Paragraph(vec![Inline::Emphasis(vec![Inline::Text(text.into())])])
    }

    /// A table of plain-text cells; the first row is the header.
    pub fn text_table<R, C>(rows: &[R]) -> Self
    where
        R: AsRef<[C]>,
        C: AsRef<str>,
    {
        Self::Table(
            rows.iter()
                .map(|row| {
                    row.as_ref()
                        .iter()
                        .map(|cell| vec![Inline::Text(cell.as_ref().to_string())])
                        .collect()
                })
                .collect(),
        )
    }

    /// Lists from a run of flat items. A list ends where the kind (bullet or
    /// numbered) changes at the same depth. An item deeper than the previous
    /// one nests under it, and a jump of several levels counts as one.
    pub fn lists(items: Vec<FlatItem>) -> Vec<Block> {
        let mut items = items.into_iter().peekable();
        let mut blocks = Vec::new();
        while let Some(first) = items.peek() {
            let base = first.depth;
            blocks.push(Self::list_at(&mut items, base));
        }
        blocks
    }

    fn list_at(
        items: &mut std::iter::Peekable<std::vec::IntoIter<FlatItem>>,
        depth: usize,
    ) -> Block {
        let ordered = items.peek().is_some_and(|i| i.ordered);
        let mut list_items: Vec<Vec<Block>> = Vec::new();
        while let Some(item) = items.peek() {
            if item.depth < depth || (item.depth == depth && item.ordered != ordered) {
                break;
            }
            if item.depth > depth {
                // Deeper than any item so far; attach to the last one.
                let nested = Self::list_at(items, depth + 1);
                match list_items.last_mut() {
                    Some(last) => last.push(nested),
                    None => list_items.push(vec![nested]),
                }
                continue;
            }
            let item = items.next().expect("peeked");
            list_items.push(vec![Block::Paragraph(item.content)]);
        }
        Block::List {
            ordered,
            items: list_items,
        }
    }
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, block: Block) {
        self.blocks.push(block);
    }

    pub fn extend(&mut self, blocks: impl IntoIterator<Item = Block>) {
        self.blocks.extend(blocks);
    }

    /// Writes the document as Markdown, one blank line between blocks.
    pub fn write_markdown(&self, writer: &mut dyn Write) -> Result<()> {
        write!(writer, "{}", render_blocks(&self.blocks))?;
        Ok(())
    }
}

/// Collects blocks while reading a markup format line by line. A run of list
/// items is flushed as lists before any other block.
#[derive(Default)]
pub struct Builder {
    doc: Document,
    items: Vec<FlatItem>,
}

impl Builder {
    fn flush(&mut self) {
        let items = std::mem::take(&mut self.items);
        self.doc.extend(Block::lists(items));
    }

    pub fn push(&mut self, block: Block) {
        self.flush();
        self.doc.push(block);
    }

    /// Adds a list item; consecutive items form one list.
    pub fn item(&mut self, depth: usize, ordered: bool, content: Vec<Inline>) {
        self.items.push(FlatItem {
            depth,
            ordered,
            content,
        });
    }

    pub fn finish(mut self) -> Document {
        self.flush();
        self.doc
    }
}

/// Blocks separated by one blank line, ending in a newline.
fn render_blocks(blocks: &[Block]) -> String {
    let rendered: Vec<String> = blocks
        .iter()
        .map(render_block)
        .filter(|s| !s.is_empty())
        .collect();
    rendered.join("\n")
}

fn render_block(block: &Block) -> String {
    match block {
        Block::Heading { level, content } => {
            let level = usize::from((*level).clamp(1, 6));
            format!("{} {}\n", "#".repeat(level), inlines(content, false))
        }
        Block::Paragraph(content) => format!("{}\n", inlines(content, false)),
        Block::List { ordered, items } => render_list(*ordered, items),
        Block::Code { lang, text } => render_code(lang.as_deref(), text),
        Block::Quote(blocks) => render_quote(blocks),
        Block::Rule => "---\n".to_string(),
        Block::Table(rows) => render_table(rows),
    }
}

fn render_list(ordered: bool, items: &[Vec<Block>]) -> String {
    let mut out = String::new();
    for (n, item) in items.iter().enumerate() {
        let marker = if ordered {
            format!("{}. ", n + 1)
        } else {
            "- ".to_string()
        };
        let indent = " ".repeat(marker.len());
        // A nested list follows its paragraph directly; other blocks get a blank line.
        let mut body = String::new();
        for (i, block) in item.iter().enumerate() {
            let tight = i > 0 && matches!(block, Block::List { .. });
            if i > 0 && !tight {
                body.push('\n');
            }
            body.push_str(&render_block(block));
        }
        for (i, line) in body.lines().enumerate() {
            match (i, line.is_empty()) {
                (0, _) => {
                    out.push_str(&marker);
                    out.push_str(line);
                }
                (_, true) => {}
                (_, false) => {
                    out.push_str(&indent);
                    out.push_str(line);
                }
            }
            out.push('\n');
        }
    }
    out
}

fn render_code(lang: Option<&str>, text: &str) -> String {
    let longest = text
        .lines()
        .map(|l| l.chars().take_while(|&c| c == '`').count())
        .max()
        .unwrap_or(0);
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}{}\n{text}\n{fence}\n", lang.unwrap_or(""))
}

fn render_quote(blocks: &[Block]) -> String {
    let mut out = String::new();
    for line in render_blocks(blocks).lines() {
        if line.is_empty() {
            out.push_str(">\n");
        } else {
            out.push_str("> ");
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn render_table(rows: &[Vec<Vec<Inline>>]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return String::new();
    }
    let mut nodes = Vec::new();
    for (row, cells) in rows.iter().enumerate() {
        for column in 0..columns {
            let content = cells
                .get(column)
                .map_or_else(String::new, |c| inlines(c, true));
            nodes.push(Node::TableCell(TableCell {
                values: cell_values(&content),
                column,
                row,
                position: None,
            }));
        }
        if row == 0 {
            nodes.push(Node::TableAlign(TableAlign {
                align: vec![TableAlignKind::None; columns],
                position: None,
            }));
        }
    }
    Markdown::new(nodes).to_string()
}

/// A hard break would end the row, so line breaks become `<br>`.
fn cell_values(content: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    for (i, line) in content.lines().enumerate() {
        if i > 0 {
            nodes.push(Node::Html(Html {
                value: "<br>".to_string(),
                position: None,
            }));
        }
        nodes.push(Node::Text(Text {
            value: line.to_string(),
            position: None,
        }));
    }
    nodes
}

/// `in_cell` leaves `|` alone, since `mq-markdown` escapes it in table cells.
fn inlines(content: &[Inline], in_cell: bool) -> String {
    let mut out = String::new();
    for (i, inline) in content.iter().enumerate() {
        // Text right after another inline still starts mid-line.
        let at_start = i == 0;
        match inline {
            Inline::Text(text) => out.push_str(&escape(text, in_cell, at_start)),
            Inline::Emphasis(inner) => out.push_str(&wrap("*", inner, in_cell)),
            Inline::Strong(inner) => out.push_str(&wrap("**", inner, in_cell)),
            Inline::Strike(inner) => out.push_str(&wrap("~~", inner, in_cell)),
            Inline::Code(code) => out.push_str(&code_span(code)),
            Inline::Link { url, content } => {
                out.push_str(&format!(
                    "[{}]({})",
                    inlines(content, in_cell),
                    destination(url)
                ));
            }
            Inline::Image { url, alt } => {
                out.push_str(&format!(
                    "![{}]({})",
                    escape(alt, in_cell, false),
                    destination(url)
                ));
            }
        }
    }
    out
}

fn wrap(mark: &str, inner: &[Inline], in_cell: bool) -> String {
    let inner = inlines(inner, in_cell);
    if inner.trim().is_empty() {
        return inner;
    }
    format!("{mark}{inner}{mark}")
}

fn code_span(code: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in code.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat(longest + 1);
    let pad = if code.starts_with('`') || code.ends_with('`') {
        " "
    } else {
        ""
    };
    format!("{fence}{pad}{code}{pad}{fence}")
}

/// A link target; wrapped in `<>` when it holds spaces or parentheses.
fn destination(url: &str) -> String {
    if url.contains([' ', '(', ')']) {
        format!("<{}>", url.replace('<', "%3C").replace('>', "%3E"))
    } else {
        url.to_string()
    }
}

/// Escapes what would otherwise change the meaning of the text. `_` is left
/// alone inside words, where it is not emphasis. `at_start` is whether the
/// text begins a line, where block markers matter.
fn escape(text: &str, in_cell: bool, at_start: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut line_start = at_start;
    for (i, &c) in chars.iter().enumerate() {
        let word = |j: Option<usize>| {
            j.and_then(|j| chars.get(j))
                .is_some_and(|c| c.is_alphanumeric())
        };
        let escaped = match c {
            '\\' | '`' | '*' | '[' | ']' | '<' => true,
            '|' => !in_cell,
            '_' => !(word(i.checked_sub(1)) && word(Some(i + 1))),
            '#' | '>' | '-' | '+' => line_start,
            '.' | ')' => leading_number(&chars, i, at_start),
            _ => false,
        };
        if escaped {
            out.push('\\');
        }
        out.push(c);
        line_start = c == '\n' || (line_start && c == ' ');
    }
    out
}

/// Whether everything before `i` on its line is digits, so the `.` or `)` would start a list.
fn leading_number(chars: &[char], i: usize, at_start: bool) -> bool {
    let newline = chars[..i].iter().rposition(|&c| c == '\n');
    if newline.is_none() && !at_start {
        return false;
    }
    let lead = &chars[newline.map_or(0, |p| p + 1)..i];
    !lead.is_empty() && lead.iter().all(char::is_ascii_digit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markdown(doc: &Document) -> String {
        let mut out = Vec::new();
        doc.write_markdown(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn doc(blocks: Vec<Block>) -> Document {
        Document { blocks }
    }

    fn t(s: &str) -> Inline {
        Inline::Text(s.to_string())
    }

    fn item(depth: usize, ordered: bool, text: &str) -> FlatItem {
        FlatItem {
            depth,
            ordered,
            content: vec![t(text)],
        }
    }

    #[test]
    fn blocks_are_separated_by_one_blank_line() {
        let d = doc(vec![
            Block::heading(1, "Title"),
            Block::paragraph("first"),
            Block::paragraph("second"),
        ]);
        assert_eq!(markdown(&d), "# Title\n\nfirst\n\nsecond\n");
    }

    #[test]
    fn a_table_does_not_run_into_the_text_before_it() {
        let d = doc(vec![
            Block::paragraph("intro"),
            Block::text_table(&[vec!["a", "b"], vec!["1", "2"]]),
            Block::paragraph("after"),
        ]);
        assert_eq!(
            markdown(&d),
            "intro\n\n| a | b |\n| - | - |\n| 1 | 2 |\n\nafter\n"
        );
    }

    #[test]
    fn short_rows_are_padded_with_empty_cells() {
        let d = doc(vec![Block::text_table(&[vec!["a", "b"], vec!["1"]])]);
        assert_eq!(markdown(&d), "| a | b |\n| - | - |\n| 1 |   |\n");
    }

    #[test]
    fn newlines_in_a_cell_become_br_and_pipes_are_escaped_once() {
        let d = doc(vec![Block::text_table(&[vec!["h"], vec!["a|b\nc"]])]);
        let out = markdown(&d);
        assert!(out.contains("| a\\|b<br>c |"), "{out}");
    }

    #[test]
    fn markdown_syntax_in_text_is_escaped() {
        let d = doc(vec![
            Block::paragraph("1. not a list"),
            Block::paragraph("# not a heading"),
            Block::paragraph("a *b* `c` [d] <e> f|g"),
            Block::paragraph("snake_case and _lead"),
        ]);
        assert_eq!(
            markdown(&d),
            "1\\. not a list\n\n\\# not a heading\n\na \\*b\\* \\`c\\` \\[d\\] \\<e> f\\|g\n\nsnake_case and \\_lead\n"
        );
    }

    #[test]
    fn an_empty_table_writes_nothing() {
        let rows: Vec<Vec<String>> = vec![];
        assert_eq!(markdown(&doc(vec![Block::text_table(&rows)])), "");
    }

    #[test]
    fn inline_marks_nest_and_wrap_their_content() {
        let d = doc(vec![Block::Paragraph(vec![
            t("a "),
            Inline::Strong(vec![t("b")]),
            t(" "),
            Inline::Emphasis(vec![t("c")]),
            t(" "),
            Inline::Code("d".into()),
            t(" "),
            Inline::Link {
                url: "https://x.io".into(),
                content: vec![t("e")],
            },
        ])]);
        assert_eq!(markdown(&d), "a **b** *c* `d` [e](https://x.io)\n");
    }

    #[test]
    fn strike_wraps_in_tildes() {
        let d = doc(vec![Block::Paragraph(vec![Inline::Strike(vec![t("x")])])]);
        assert_eq!(markdown(&d), "~~x~~\n");
    }

    #[test]
    fn code_spans_and_fences_outgrow_the_backticks_inside() {
        assert_eq!(code_span("a`b"), "``a`b``");
        assert_eq!(code_span("`x"), "`` `x ``");
        let d = doc(vec![Block::Code {
            lang: Some("md".into()),
            text: "```\ninner\n```".into(),
        }]);
        assert_eq!(markdown(&d), "````md\n```\ninner\n```\n````\n");
    }

    #[test]
    fn link_destinations_with_spaces_are_wrapped() {
        assert_eq!(destination("a b"), "<a b>");
        assert_eq!(destination("https://x.io/a(b)"), "<https://x.io/a(b)>");
        assert_eq!(destination("https://x.io"), "https://x.io");
    }

    #[test]
    fn lists_are_tight_and_numbered_in_order() {
        let d = doc(Block::lists(vec![
            item(0, true, "one"),
            item(0, true, "two"),
        ]));
        assert_eq!(markdown(&d), "1. one\n2. two\n");
    }

    #[test]
    fn nested_items_indent_under_their_parent() {
        let d = doc(Block::lists(vec![
            item(0, false, "a"),
            item(1, true, "a1"),
            item(1, true, "a2"),
            item(0, false, "b"),
        ]));
        assert_eq!(markdown(&d), "- a\n  1. a1\n  2. a2\n- b\n");
    }

    #[test]
    fn a_change_of_kind_at_the_same_depth_starts_a_new_list() {
        let d = doc(Block::lists(vec![item(0, false, "a"), item(0, true, "b")]));
        assert_eq!(markdown(&d), "- a\n\n1. b\n");
    }

    #[test]
    fn a_list_is_followed_by_a_blank_line() {
        let d = doc(vec![
            Block::lists(vec![item(0, false, "a")]).remove(0),
            Block::paragraph("after"),
        ]);
        assert_eq!(markdown(&d), "- a\n\nafter\n");
    }

    #[test]
    fn quotes_prefix_every_line_and_keep_blank_lines() {
        let d = doc(vec![Block::Quote(vec![
            Block::paragraph("one"),
            Block::paragraph("two"),
        ])]);
        assert_eq!(markdown(&d), "> one\n>\n> two\n");
    }

    #[test]
    fn rule_is_three_dashes() {
        let d = doc(vec![
            Block::paragraph("a"),
            Block::Rule,
            Block::paragraph("b"),
        ]);
        assert_eq!(markdown(&d), "a\n\n---\n\nb\n");
    }
}
