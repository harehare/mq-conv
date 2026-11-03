//! Builders for the Markdown AST that converters produce.
//!
//! Converters build [`Node`]s and [`write`] renders them, so escaping and
//! table layout live in `mq-markdown` instead of in every format.

use std::io::Write;

use mq_markdown::{Emphasis, Html, Markdown, Node, TableAlign, TableAlignKind, TableCell, Text};

use crate::error::Result;

/// A plain text node.
pub fn text(value: impl Into<String>) -> Node {
    Node::Text(Text {
        value: value.into(),
        position: None,
    })
}

/// An emphasized paragraph, e.g. `*Empty CSV*`.
pub fn emphasis(value: impl Into<String>) -> Node {
    Node::Emphasis(Emphasis {
        values: vec![text(value)],
        position: None,
    })
}

/// Table cell content. Line breaks become `<br>`, because a hard break would end the row.
fn cell_values(value: &str) -> Vec<Node> {
    let mut nodes = Vec::new();
    for (i, line) in value.lines().enumerate() {
        if i > 0 {
            nodes.push(Node::Html(Html {
                value: "<br>".to_string(),
                position: None,
            }));
        }
        nodes.push(text(line));
    }
    nodes
}

/// A table whose first row is the header. Short rows are padded with empty
/// cells; returns no nodes when there are no columns.
pub fn table<R, C>(rows: &[R]) -> Vec<Node>
where
    R: AsRef<[C]>,
    C: AsRef<str>,
{
    let columns = rows.iter().map(|r| r.as_ref().len()).max().unwrap_or(0);
    let mut nodes = Vec::new();
    for (row, cells) in rows.iter().enumerate() {
        let cells = cells.as_ref();
        for column in 0..columns {
            let value = cells.get(column).map_or("", AsRef::as_ref);
            nodes.push(Node::TableCell(TableCell {
                values: cell_values(value),
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
    nodes
}

/// Renders `nodes` as Markdown.
pub fn write(writer: &mut dyn Write, nodes: Vec<Node>) -> Result<()> {
    write!(writer, "{}", Markdown::new(nodes))?;
    Ok(())
}
