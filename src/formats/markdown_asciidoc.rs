use std::io::Write;

use mq_markdown::{Markdown, Node};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct MarkdownAsciidocConverter;

impl Converter for MarkdownAsciidocConverter {
    fn format_name(&self) -> &'static str {
        "markdown-asciidoc"
    }

    fn output_extension(&self) -> &'static str {
        "adoc"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let markdown = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "markdown-asciidoc",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        let parsed = markdown.parse::<Markdown>().map_err(|e| Error::Conversion {
            format: "markdown-asciidoc",
            message: e.to_string(),
        })?;

        write_asciidoc(&parsed.nodes, writer).map_err(|e| Error::Conversion {
            format: "markdown-asciidoc",
            message: e.to_string(),
        })?;

        Ok(())
    }
}

fn inline_to_asciidoc(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&t.value),
            Node::Strong(s) => {
                out.push('*');
                out.push_str(&inline_to_asciidoc(&s.values));
                out.push('*');
            }
            Node::Emphasis(e) => {
                out.push('_');
                out.push_str(&inline_to_asciidoc(&e.values));
                out.push('_');
            }
            Node::CodeInline(c) => {
                out.push('`');
                out.push_str(&c.value);
                out.push('`');
            }
            Node::Link(l) => {
                let url = l.url.as_str();
                let text = inline_to_asciidoc(&l.values);
                out.push_str(&format!("{url}[{text}]"));
            }
            Node::Delete(d) => {
                out.push_str("[line-through]#");
                out.push_str(&inline_to_asciidoc(&d.values));
                out.push('#');
            }
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

fn write_asciidoc(nodes: &[Node], writer: &mut dyn Write) -> std::io::Result<()> {
    let mut table_data: Vec<(usize, usize, String)> = Vec::new();

    macro_rules! flush_table {
        () => {
            if !table_data.is_empty() {
                let max_col = table_data.iter().map(|(_, c, _)| *c).max().unwrap_or(0) + 1;
                let max_row = table_data.iter().map(|(r, _, _)| *r).max().unwrap_or(0);
                writeln!(writer, "[%header,cols=\"{}\"]", "1,".repeat(max_col).trim_end_matches(','))?;
                writeln!(writer, "|===")?;
                for row_idx in 0..=max_row {
                    let mut cells = Vec::new();
                    for col_idx in 0..max_col {
                        let text = table_data
                            .iter()
                            .find(|(r, c, _)| *r == row_idx && *c == col_idx)
                            .map(|(_, _, t)| t.as_str())
                            .unwrap_or("");
                        cells.push(format!("| {text}"));
                    }
                    writeln!(writer, "{}", cells.join(" "))?;
                }
                writeln!(writer, "|===")?;
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
                let prefix = "=".repeat(h.depth as usize);
                let text = inline_to_asciidoc(&h.values);
                writeln!(writer, "{prefix} {text}")?;
                writeln!(writer)?;
            }

            Node::Code(c) => {
                if let Some(lang) = &c.lang {
                    writeln!(writer, "[source,{lang}]")?;
                }
                writeln!(writer, "----")?;
                writeln!(writer, "{}", c.value)?;
                writeln!(writer, "----")?;
                writeln!(writer)?;
            }

            Node::List(l) => {
                let text = inline_to_asciidoc(&l.values);
                if l.ordered {
                    writeln!(writer, ". {text}")?;
                } else {
                    writeln!(writer, "* {text}")?;
                }
            }

            Node::Blockquote(bq) => {
                let text = inline_to_asciidoc(&bq.values);
                writeln!(writer, "[quote]")?;
                writeln!(writer, "____")?;
                writeln!(writer, "{text}")?;
                writeln!(writer, "____")?;
                writeln!(writer)?;
            }

            Node::HorizontalRule(_) => {
                writeln!(writer, "'''")?;
                writeln!(writer)?;
            }

            Node::Text(_)
            | Node::Strong(_)
            | Node::Emphasis(_)
            | Node::CodeInline(_)
            | Node::Break(_)
            | Node::Link(_)
            | Node::Delete(_) => {
                let text = inline_to_asciidoc(std::slice::from_ref(node));
                writeln!(writer, "{text}")?;
                writeln!(writer)?;
            }

            _ => {}
        }
    }

    flush_table!();

    Ok(())
}
