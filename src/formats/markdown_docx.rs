use std::io::{Cursor, Write};

use docx_rs::{
    AbstractNumbering, AlignmentType, Docx, IndentLevel, Level, LevelJc, LevelText, NumberFormat,
    Numbering, NumberingId, Paragraph, Run, Start, Table, TableRow,
};
use mq_markdown::{Markdown, Node};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct MarkdownDocxConverter;

impl Converter for MarkdownDocxConverter {
    fn format_name(&self) -> &'static str {
        "markdown-docx"
    }

    fn output_extension(&self) -> &'static str {
        "docx"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let markdown = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "markdown-docx",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        let doc = build_docx(markdown).map_err(|e| Error::Conversion {
            format: "markdown-docx",
            message: e.to_string(),
        })?;

        let mut buf = Cursor::new(Vec::new());
        doc.build().pack(&mut buf).map_err(|e| Error::Conversion {
            format: "markdown-docx",
            message: format!("Failed to generate docx: {e}"),
        })?;

        writer.write_all(buf.get_ref())?;
        Ok(())
    }
}

fn heading_style(depth: u8) -> &'static str {
    match depth {
        1 => "Heading1",
        2 => "Heading2",
        3 => "Heading3",
        4 => "Heading4",
        5 => "Heading5",
        _ => "Heading6",
    }
}

// (text, bold, italic, code)
type RunInfo = (String, bool, bool, bool);

fn collect_runs(values: &[Node], bold: bool, italic: bool) -> Vec<RunInfo> {
    let mut runs = Vec::new();
    for v in values {
        match v {
            Node::Text(t) => runs.push((t.value.clone(), bold, italic, false)),
            Node::CodeInline(c) => runs.push((c.value.to_string(), false, false, true)),
            Node::Strong(s) => runs.extend(collect_runs(&s.values, true, italic)),
            Node::Emphasis(e) => runs.extend(collect_runs(&e.values, bold, true)),
            Node::Break(_) => runs.push((" ".to_string(), false, false, false)),
            Node::Link(l) => runs.extend(collect_runs(&l.values, bold, italic)),
            Node::Delete(d) => runs.extend(collect_runs(&d.values, bold, italic)),
            _ => {}
        }
    }
    runs
}

fn extract_text(values: &[Node]) -> String {
    collect_runs(values, false, false)
        .into_iter()
        .map(|(t, _, _, _)| t)
        .collect()
}

fn build_paragraph_from_runs(runs: Vec<RunInfo>) -> Paragraph {
    let mut para = Paragraph::new();
    for (text, bold, italic, code) in runs {
        let mut run = Run::new().add_text(&text);
        if bold {
            run = run.bold();
        }
        if italic {
            run = run.italic();
        }
        if code {
            run = run.fonts(docx_rs::RunFonts::new().ascii("Courier New"));
        }
        para = para.add_run(run);
    }
    para
}

fn flush_inline_runs(doc: Docx, runs: &mut Vec<RunInfo>) -> Docx {
    if runs.is_empty() {
        return doc;
    }
    let para = build_paragraph_from_runs(std::mem::take(runs));
    doc.add_paragraph(para)
}

fn flush_table(doc: Docx, table_data: &mut Vec<(usize, usize, String)>) -> Docx {
    if table_data.is_empty() {
        return doc;
    }
    let max_row = table_data.iter().map(|(r, _, _)| *r).max().unwrap_or(0);
    let col_count = table_data.iter().map(|(_, c, _)| *c).max().unwrap_or(0) + 1;

    let mut table = Table::new(vec![]);
    for row_idx in 0..=max_row {
        let mut cells = vec![];
        for col_idx in 0..col_count {
            let text = table_data
                .iter()
                .find(|(r, c, _)| *r == row_idx && *c == col_idx)
                .map(|(_, _, t)| t.as_str())
                .unwrap_or("");
            let mut run = Run::new().add_text(text);
            if row_idx == 0 {
                run = run.bold();
            }
            let cell = docx_rs::TableCell::new()
                .add_paragraph(Paragraph::new().align(AlignmentType::Left).add_run(run));
            cells.push(cell);
        }
        table = table.add_row(TableRow::new(cells));
    }
    table_data.clear();
    doc.add_table(table)
}

fn build_docx(markdown: &str) -> std::result::Result<Docx, Box<dyn std::error::Error>> {
    let mut doc = Docx::new();

    // Add a list numbering definition for unordered lists
    let abstract_numbering = AbstractNumbering::new(0).add_level(
        Level::new(
            0,
            Start::new(1),
            NumberFormat::new("bullet"),
            LevelText::new("•"),
            LevelJc::new("left"),
        )
        .indent(
            Some(720),
            Some(docx_rs::SpecialIndentType::Hanging(360)),
            None,
            None,
        ),
    );
    doc = doc.add_abstract_numbering(abstract_numbering);
    doc = doc.add_numbering(Numbering::new(1, 0));

    let parsed = markdown.parse::<Markdown>()?;

    let mut inline_runs: Vec<RunInfo> = Vec::new();
    let mut prev_end_line: Option<usize> = None;
    let mut table_data: Vec<(usize, usize, String)> = Vec::new();
    let mut in_table = false;
    let mut ordered_item_count: u64 = 0;
    let mut in_ordered_list = false;

    for node in &parsed.nodes {
        match node {
            Node::TableCell(cell) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
                prev_end_line = None;
                in_table = true;
                in_ordered_list = false;
                ordered_item_count = 0;
                let text = extract_text(&cell.values);
                table_data.push((cell.row, cell.column, text));
                continue;
            }
            Node::TableAlign(_) => {
                continue;
            }
            _ => {
                if in_table {
                    doc = flush_table(doc, &mut table_data);
                    in_table = false;
                }
            }
        }

        match node {
            Node::Heading(h) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
                in_ordered_list = false;
                ordered_item_count = 0;
                let text = extract_text(&h.values);
                let para = Paragraph::new()
                    .style(heading_style(h.depth))
                    .add_run(Run::new().add_text(&text));
                doc = doc.add_paragraph(para);
                prev_end_line = h.position.as_ref().map(|p| p.end.line);
            }

            Node::Code(c) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
                in_ordered_list = false;
                ordered_item_count = 0;
                for line in c.value.lines() {
                    let para = Paragraph::new().add_run(
                        Run::new()
                            .add_text(line)
                            .fonts(docx_rs::RunFonts::new().ascii("Courier New")),
                    );
                    doc = doc.add_paragraph(para);
                }
                prev_end_line = c.position.as_ref().map(|p| p.end.line);
            }

            Node::List(l) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
                let text = extract_text(&l.values);
                if l.ordered {
                    if !in_ordered_list {
                        ordered_item_count = 1;
                        in_ordered_list = true;
                    } else {
                        ordered_item_count += 1;
                    }
                    let formatted = format!("{ordered_item_count}. {text}");
                    let para = Paragraph::new().add_run(Run::new().add_text(&formatted));
                    doc = doc.add_paragraph(para);
                } else {
                    in_ordered_list = false;
                    ordered_item_count = 0;
                    let para = Paragraph::new()
                        .numbering(NumberingId::new(1), IndentLevel::new(0))
                        .add_run(Run::new().add_text(&text));
                    doc = doc.add_paragraph(para);
                }
                prev_end_line = l.position.as_ref().map(|p| p.end.line);
            }

            Node::Blockquote(bq) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
                in_ordered_list = false;
                ordered_item_count = 0;
                let text = extract_text(&bq.values);
                let para = Paragraph::new()
                    .style("Quote")
                    .add_run(Run::new().add_text(&text));
                doc = doc.add_paragraph(para);
                prev_end_line = bq.position.as_ref().map(|p| p.end.line);
            }

            Node::HorizontalRule(_) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
                in_ordered_list = false;
                ordered_item_count = 0;
                let para = Paragraph::new().add_run(Run::new().add_text("─".repeat(40)));
                doc = doc.add_paragraph(para);
                prev_end_line = None;
            }

            // Inline nodes — group into paragraphs using position info
            Node::Text(_)
            | Node::Strong(_)
            | Node::Emphasis(_)
            | Node::CodeInline(_)
            | Node::Break(_)
            | Node::Link(_)
            | Node::Delete(_) => {
                in_ordered_list = false;
                ordered_item_count = 0;
                if let Some(pos) = node.position()
                    && let Some(end) = prev_end_line
                {
                    if pos.start.line > end + 1 {
                        doc = flush_inline_runs(doc, &mut inline_runs);
                    }
                    prev_end_line = Some(pos.end.line);
                }
                let runs = match node {
                    Node::Text(t) => vec![(t.value.clone(), false, false, false)],
                    Node::Strong(s) => collect_runs(&s.values, true, false),
                    Node::Emphasis(e) => collect_runs(&e.values, false, true),
                    Node::CodeInline(c) => vec![(c.value.to_string(), false, false, true)],
                    Node::Break(_) => vec![(" ".to_string(), false, false, false)],
                    Node::Link(l) => collect_runs(&l.values, false, false),
                    Node::Delete(d) => collect_runs(&d.values, false, false),
                    _ => vec![],
                };
                inline_runs.extend(runs);
            }

            _ => {}
        }
    }

    if in_table {
        doc = flush_table(doc, &mut table_data);
    }
    doc = flush_inline_runs(doc, &mut inline_runs);

    Ok(doc)
}
