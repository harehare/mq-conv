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

const MAX_LIST_LEVEL: usize = 8;
const BULLET_CHARS: [&str; 3] = ["•", "◦", "▪"];

fn bullet_level(level: usize) -> Level {
    let indent = 720 * (level as i32 + 1);
    Level::new(
        level,
        Start::new(1),
        NumberFormat::new("bullet"),
        LevelText::new(BULLET_CHARS[level % BULLET_CHARS.len()]),
        LevelJc::new("left"),
    )
    .indent(
        Some(indent),
        Some(docx_rs::SpecialIndentType::Hanging(360)),
        None,
        None,
    )
}

fn decimal_level(level: usize) -> Level {
    let indent = 720 * (level as i32 + 1);
    Level::new(
        level,
        Start::new(1),
        NumberFormat::new("decimal"),
        LevelText::new(format!("%{}.", level + 1)),
        LevelJc::new("left"),
    )
    .indent(
        Some(indent),
        Some(docx_rs::SpecialIndentType::Hanging(360)),
        None,
        None,
    )
}

fn build_docx(markdown: &str) -> std::result::Result<Docx, Box<dyn std::error::Error>> {
    let mut doc = Docx::new();

    let mut bullet_numbering = AbstractNumbering::new(0);
    let mut decimal_numbering = AbstractNumbering::new(1);
    for level in 0..=MAX_LIST_LEVEL {
        bullet_numbering = bullet_numbering.add_level(bullet_level(level));
        decimal_numbering = decimal_numbering.add_level(decimal_level(level));
    }
    doc = doc.add_abstract_numbering(bullet_numbering);
    doc = doc.add_abstract_numbering(decimal_numbering);
    doc = doc.add_numbering(Numbering::new(1, 0));
    doc = doc.add_numbering(Numbering::new(2, 1));

    let parsed = markdown.parse::<Markdown>()?;

    let mut inline_runs: Vec<RunInfo> = Vec::new();
    let mut prev_end_line: Option<usize> = None;
    let mut table_data: Vec<(usize, usize, String)> = Vec::new();
    let mut in_table = false;

    for node in &parsed.nodes {
        match node {
            Node::TableCell(cell) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
                prev_end_line = None;
                in_table = true;
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
                let text = extract_text(&h.values);
                let para = Paragraph::new()
                    .style(heading_style(h.depth))
                    .add_run(Run::new().add_text(&text));
                doc = doc.add_paragraph(para);
                prev_end_line = h.position.as_ref().map(|p| p.end.line);
            }

            Node::Code(c) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
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
                let mut text = extract_text(&l.values);
                if let Some(checked) = l.checked {
                    text = format!("{} {text}", if checked { "☑" } else { "☐" });
                }
                let level = (l.level as usize).min(MAX_LIST_LEVEL);
                let numbering_id = if l.ordered { 2 } else { 1 };
                let para = Paragraph::new()
                    .numbering(NumberingId::new(numbering_id), IndentLevel::new(level))
                    .add_run(Run::new().add_text(&text));
                doc = doc.add_paragraph(para);
                prev_end_line = l.position.as_ref().map(|p| p.end.line);
            }

            Node::Blockquote(bq) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
                let text = extract_text(&bq.values);
                let para = Paragraph::new()
                    .style("Quote")
                    .add_run(Run::new().add_text(&text));
                doc = doc.add_paragraph(para);
                prev_end_line = bq.position.as_ref().map(|p| p.end.line);
            }

            Node::HorizontalRule(_) => {
                doc = flush_inline_runs(doc, &mut inline_runs);
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

#[cfg(all(test, feature = "zip"))]
mod tests {
    use super::*;
    use std::io::Read;

    fn document_xml(markdown: &str) -> String {
        let mut out = Vec::new();
        MarkdownDocxConverter
            .convert(markdown.as_bytes(), &mut out)
            .unwrap();
        let cursor = Cursor::new(out);
        let mut archive = zip::ZipArchive::new(cursor).unwrap();
        let mut xml = String::new();
        archive
            .by_name("word/document.xml")
            .unwrap()
            .read_to_string(&mut xml)
            .unwrap();
        xml
    }

    #[test]
    fn test_nested_bullet_list_uses_distinct_indent_levels() {
        let xml = document_xml("- Top\n  - Sub\n");
        assert!(xml.contains(r#"<w:ilvl w:val="0""#), "{xml}");
        assert!(xml.contains(r#"<w:ilvl w:val="1""#), "{xml}");
    }

    #[test]
    fn test_ordered_list_uses_decimal_numbering() {
        let xml = document_xml("1. First\n2. Second\n");
        assert!(xml.contains(r#"<w:numId w:val="2""#), "{xml}");
        assert!(xml.contains("First"));
        assert!(xml.contains("Second"));
    }

    #[test]
    fn test_bullet_list_uses_bullet_numbering() {
        let xml = document_xml("- Item\n");
        assert!(xml.contains(r#"<w:numId w:val="1""#), "{xml}");
    }

    #[test]
    fn test_task_list_shows_checked_state() {
        let xml = document_xml("- [x] Done\n- [ ] Todo\n");
        assert!(xml.contains("☑ Done"), "{xml}");
        assert!(xml.contains("☐ Todo"), "{xml}");
    }
}
