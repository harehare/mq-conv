use std::io::Write;

use mq_markdown::{Markdown, Node};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct MarkdownRstConverter;

impl Converter for MarkdownRstConverter {
    fn format_name(&self) -> &'static str {
        "markdown-rst"
    }

    fn output_extension(&self) -> &'static str {
        "rst"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let markdown = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "markdown-rst",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        let parsed = markdown.parse::<Markdown>().map_err(|e| Error::Conversion {
            format: "markdown-rst",
            message: e.to_string(),
        })?;

        write_rst(&parsed.nodes, writer).map_err(|e| Error::Conversion {
            format: "markdown-rst",
            message: e.to_string(),
        })?;

        Ok(())
    }
}

fn heading_char(depth: u8) -> char {
    match depth {
        1 => '=',
        2 => '-',
        3 => '~',
        4 => '^',
        _ => '"',
    }
}

fn inline_to_rst(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&t.value),
            Node::Strong(s) => {
                out.push_str("**");
                out.push_str(&inline_to_rst(&s.values));
                out.push_str("**");
            }
            Node::Emphasis(e) => {
                out.push('*');
                out.push_str(&inline_to_rst(&e.values));
                out.push('*');
            }
            Node::CodeInline(c) => {
                out.push_str("``");
                out.push_str(&c.value);
                out.push_str("``");
            }
            Node::Link(l) => {
                let text = inline_to_rst(&l.values);
                let url = l.url.as_str();
                out.push_str(&format!("`{text} <{url}>`_"));
            }
            Node::Delete(d) => out.push_str(&inline_to_rst(&d.values)),
            Node::Break(_) => out.push(' '),
            _ => {}
        }
    }
    out
}

fn extract_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&t.value),
            Node::Strong(s) => out.push_str(&extract_text(&s.values)),
            Node::Emphasis(e) => out.push_str(&extract_text(&e.values)),
            Node::CodeInline(c) => out.push_str(&c.value),
            Node::Link(l) => out.push_str(&extract_text(&l.values)),
            Node::Delete(d) => out.push_str(&extract_text(&d.values)),
            Node::Break(_) => out.push(' '),
            _ => {}
        }
    }
    out
}

fn write_rst(nodes: &[Node], writer: &mut dyn Write) -> std::io::Result<()> {
    let mut table_data: Vec<(usize, usize, String)> = Vec::new();

    macro_rules! flush_table {
        () => {
            if !table_data.is_empty() {
                let max_col = table_data.iter().map(|(_, c, _)| *c).max().unwrap_or(0) + 1;
                let max_row = table_data.iter().map(|(r, _, _)| *r).max().unwrap_or(0);
                writeln!(writer, ".. list-table::")?;
                writeln!(writer, "   :header-rows: 1")?;
                writeln!(writer)?;
                for row_idx in 0..=max_row {
                    for col_idx in 0..max_col {
                        let text = table_data
                            .iter()
                            .find(|(r, c, _)| *r == row_idx && *c == col_idx)
                            .map(|(_, _, t)| t.as_str())
                            .unwrap_or("");
                        if col_idx == 0 {
                            writeln!(writer, "   * - {text}")?;
                        } else {
                            writeln!(writer, "     - {text}")?;
                        }
                    }
                }
                writeln!(writer)?;
                table_data.clear();
            }
        };
    }

    for node in nodes {
        match node {
            Node::TableCell(cell) => {
                let text = extract_text(&cell.values);
                table_data.push((cell.row, cell.column, text));
                continue;
            }
            Node::TableAlign(_) => continue,
            _ => {
                flush_table!();
            }
        }

        match node {
            Node::Heading(h) => {
                let text = extract_text(&h.values);
                let underline = heading_char(h.depth).to_string().repeat(text.chars().count().max(1));
                writeln!(writer, "{text}")?;
                writeln!(writer, "{underline}")?;
                writeln!(writer)?;
            }

            Node::Code(c) => {
                if let Some(lang) = &c.lang {
                    writeln!(writer, ".. code-block:: {lang}")?;
                } else {
                    writeln!(writer, ".. code-block::")?;
                }
                writeln!(writer)?;
                for line in c.value.lines() {
                    writeln!(writer, "   {line}")?;
                }
                writeln!(writer)?;
            }

            Node::List(l) => {
                let text = inline_to_rst(&l.values);
                if l.ordered {
                    writeln!(writer, "#. {text}")?;
                } else {
                    writeln!(writer, "* {text}")?;
                }
            }

            Node::Blockquote(bq) => {
                let text = inline_to_rst(&bq.values);
                for line in text.lines() {
                    writeln!(writer, "   {line}")?;
                }
                writeln!(writer)?;
            }

            Node::HorizontalRule(_) => {
                writeln!(writer, "----")?;
                writeln!(writer)?;
            }

            Node::Text(_)
            | Node::Strong(_)
            | Node::Emphasis(_)
            | Node::CodeInline(_)
            | Node::Break(_)
            | Node::Link(_)
            | Node::Delete(_) => {
                let text = inline_to_rst(std::slice::from_ref(node));
                writeln!(writer, "{text}")?;
                writeln!(writer)?;
            }

            _ => {}
        }
    }

    flush_table!();

    Ok(())
}
