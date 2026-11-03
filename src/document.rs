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
    /// Rows of cells; the first row is the header.
    Table(Vec<Vec<Vec<Inline>>>),
}

/// An inline element.
#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    Text(String),
    Emphasis(Vec<Inline>),
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
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, block: Block) {
        self.blocks.push(block);
    }

    /// Writes the document as Markdown, one blank line between blocks.
    pub fn write_markdown(&self, writer: &mut dyn Write) -> Result<()> {
        for (i, block) in self.blocks.iter().enumerate() {
            if i > 0 {
                writeln!(writer)?;
            }
            write!(writer, "{}", render_block(block))?;
        }
        Ok(())
    }
}

fn render_block(block: &Block) -> String {
    match block {
        Block::Heading { level, content } => {
            let level = usize::from((*level).clamp(1, 6));
            format!("{} {}\n", "#".repeat(level), inlines(content, false))
        }
        Block::Paragraph(content) => format!("{}\n", inlines(content, false)),
        Block::Table(rows) => render_table(rows),
    }
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
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text(text) => escape(text, in_cell),
            Inline::Emphasis(inner) => format!("*{}*", inlines(inner, in_cell)),
        })
        .collect()
}

/// Escapes what would otherwise change the meaning of the text. `_` is left
/// alone inside words, where it is not emphasis.
fn escape(text: &str, in_cell: bool) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut line_start = true;
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
            '.' | ')' => line_start_number(&chars, i),
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

/// Whether the `.` or `)` at `i` ends a leading number, which would start a list.
fn line_start_number(chars: &[char], i: usize) -> bool {
    let start = chars[..i]
        .iter()
        .rposition(|&c| c == '\n')
        .map_or(0, |p| p + 1);
    let lead = &chars[start..i];
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
    fn emphasis_wraps_its_content() {
        assert_eq!(
            markdown(&doc(vec![Block::emphasized("Empty sheet")])),
            "*Empty sheet*\n"
        );
    }

    #[test]
    fn an_empty_table_writes_nothing() {
        let rows: Vec<Vec<String>> = vec![];
        assert_eq!(markdown(&doc(vec![Block::text_table(&rows)])), "");
    }
}
