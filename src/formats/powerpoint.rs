use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct PowerPointConverter;

impl Converter for PowerPointConverter {
    fn format_name(&self) -> &'static str {
        "powerpoint"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let cursor = Cursor::new(input);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| Error::Conversion {
            format: "powerpoint",
            message: e.to_string(),
        })?;

        let mut slide_names: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let name = entry.name().to_string();
                if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                    slide_names.push(name);
                }
            }
        }

        slide_names.sort_by_key(|name| {
            name.trim_start_matches("ppt/slides/slide")
                .trim_end_matches(".xml")
                .parse::<u32>()
                .unwrap_or(0)
        });

        for (idx, slide_name) in slide_names.iter().enumerate() {
            let xml = read_entry(&mut archive, slide_name)?;
            let content = extract_slide_content(&xml)?;

            if idx > 0 {
                writeln!(writer)?;
                writeln!(writer, "---")?;
                writeln!(writer)?;
            }

            // Use first shape as slide title if it looks like a title
            let mut title_written = false;
            if let Some(first) = content.shapes.first()
                && first.is_title {
                    let text = join_paragraphs_inline(&first.paragraphs);
                    writeln!(writer, "# {text}")?;
                    writeln!(writer)?;
                    title_written = true;
                }

            if !title_written {
                writeln!(writer, "# Slide {}", idx + 1)?;
                writeln!(writer)?;
            }

            let start = if title_written { 1 } else { 0 };
            let content_shapes: Vec<_> = content.shapes[start..]
                .iter()
                .filter(|s| !s.paragraphs.is_empty())
                .collect();

            if content_shapes.is_empty() && content.tables.is_empty() && !title_written {
                writeln!(writer, "*Empty slide*")?;
            }

            for shape in &content_shapes {
                if shape.is_subtitle {
                    let text = join_paragraphs_inline(&shape.paragraphs);
                    if !text.is_empty() {
                        writeln!(writer, "## {text}")?;
                        writeln!(writer)?;
                    }
                } else {
                    for para in &shape.paragraphs {
                        let text = render_paragraph(para);
                        let text = text.trim();
                        if text.is_empty() {
                            continue;
                        }

                        if shape.has_bullets {
                            writeln!(writer, "- {text}")?;
                        } else {
                            writeln!(writer, "{text}")?;
                            writeln!(writer)?;
                        }
                    }
                    if shape.has_bullets {
                        writeln!(writer)?;
                    }
                }
            }

            // Write tables
            for table in &content.tables {
                write_table(writer, table)?;
                writeln!(writer)?;
            }

            // Speaker notes
            let notes_name =
                slide_name.replace("ppt/slides/slide", "ppt/notesSlides/notesSlide");
            if let Ok(notes_xml) = read_entry(&mut archive, &notes_name) {
                let notes_content = extract_slide_content(&notes_xml)?;
                let notes_text: String = notes_content
                    .shapes
                    .iter()
                    .flat_map(|s| &s.paragraphs)
                    .map(render_paragraph)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && !s.chars().all(|c| c.is_ascii_digit()))
                    .collect::<Vec<_>>()
                    .join("\n");
                if !notes_text.is_empty() {
                    writeln!(writer, "> **Notes**: {notes_text}")?;
                    writeln!(writer)?;
                }
            }
        }

        Ok(())
    }
}

struct SlideContent {
    shapes: Vec<SlideShape>,
    tables: Vec<Vec<Vec<String>>>,
}

struct SlideShape {
    paragraphs: Vec<Paragraph>,
    is_title: bool,
    is_subtitle: bool,
    has_bullets: bool,
}

struct Paragraph {
    runs: Vec<TextRun>,
}

struct TextRun {
    text: String,
    bold: bool,
    italic: bool,
}

fn render_paragraph(para: &Paragraph) -> String {
    para.runs
        .iter()
        .map(|run| format_run_text(&run.text, run.bold, run.italic))
        .collect::<String>()
}

fn join_paragraphs_inline(paragraphs: &[Paragraph]) -> String {
    paragraphs
        .iter()
        .map(render_paragraph)
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_run_text(text: &str, bold: bool, italic: bool) -> String {
    if text.is_empty() {
        return String::new();
    }
    match (bold, italic) {
        (true, true) => format!("***{text}***"),
        (true, false) => format!("**{text}**"),
        (false, true) => format!("*{text}*"),
        (false, false) => text.to_string(),
    }
}

fn extract_slide_content(xml: &str) -> Result<SlideContent> {
    let mut shapes = Vec::new();
    let mut tables: Vec<Vec<Vec<String>>> = Vec::new();
    let mut reader = Reader::from_str(xml);

    let mut in_shape = false;
    let mut in_text_body = false;
    let mut in_paragraph = false;
    let mut in_run = false;
    let mut in_text = false;
    let mut in_ppr = false;
    let mut in_rpr = false;
    let mut in_table = false;
    let mut in_table_row = false;
    let mut in_table_cell = false;

    let mut current_run = TextRun {
        text: String::new(),
        bold: false,
        italic: false,
    };
    let mut current_paragraph = Paragraph { runs: Vec::new() };
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut shape_type = String::new();
    let mut has_bullets = false;

    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row: Vec<String> = Vec::new();
    let mut cell_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "sp" | "pic" if !in_table => {
                        in_shape = true;
                        paragraphs.clear();
                        shape_type.clear();
                        has_bullets = false;
                    }
                    "txBody" => in_text_body = true,
                    "p" if in_text_body => {
                        in_paragraph = true;
                        current_paragraph = Paragraph { runs: Vec::new() };
                    }
                    "pPr" if in_paragraph => in_ppr = true,
                    "r" if in_paragraph => {
                        in_run = true;
                        current_run = TextRun {
                            text: String::new(),
                            bold: false,
                            italic: false,
                        };
                    }
                    "rPr" if in_run => {
                        in_rpr = true;
                        // Check attributes for bold/italic
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"b" => {
                                    current_run.bold =
                                        attr.value.as_ref() == b"1" || attr.value.as_ref() == b"true";
                                }
                                b"i" => {
                                    current_run.italic =
                                        attr.value.as_ref() == b"1" || attr.value.as_ref() == b"true";
                                }
                                _ => {}
                            }
                        }
                    }
                    "t" if in_run => in_text = true,
                    "tbl" => {
                        in_table = true;
                        table_rows.clear();
                    }
                    "tr" if in_table => {
                        in_table_row = true;
                        table_row.clear();
                    }
                    "tc" if in_table_row => {
                        in_table_cell = true;
                        cell_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "ph" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"type" {
                                shape_type =
                                    String::from_utf8_lossy(&attr.value).to_string();
                            }
                        }
                        if shape_type.is_empty() {
                            shape_type = "body".to_string();
                        }
                    }
                    "buChar" | "buAutoNum" | "buFont" if in_ppr => {
                        has_bullets = true;
                    }
                    "rPr" if in_run => {
                        // Self-closing rPr
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"b" => {
                                    current_run.bold =
                                        attr.value.as_ref() == b"1" || attr.value.as_ref() == b"true";
                                }
                                b"i" => {
                                    current_run.italic =
                                        attr.value.as_ref() == b"1" || attr.value.as_ref() == b"true";
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let decoded = e.decode().unwrap_or_default().to_string();
                if in_table_cell {
                    cell_text.push_str(&decoded);
                } else if in_text {
                    current_run.text.push_str(&decoded);
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "sp" | "pic" if !in_table => {
                        if in_shape && !paragraphs.is_empty() {
                            let is_title = matches!(
                                shape_type.as_str(),
                                "title" | "ctrTitle"
                            );
                            let is_subtitle = matches!(
                                shape_type.as_str(),
                                "subTitle"
                            );
                            shapes.push(SlideShape {
                                paragraphs: std::mem::take(&mut paragraphs),
                                is_title,
                                is_subtitle,
                                has_bullets,
                            });
                        }
                        in_shape = false;
                    }
                    "txBody" => in_text_body = false,
                    "p" if in_text_body && !in_table_cell => {
                        if in_paragraph && !current_paragraph.runs.is_empty() {
                            paragraphs.push(std::mem::replace(
                                &mut current_paragraph,
                                Paragraph { runs: Vec::new() },
                            ));
                        }
                        in_paragraph = false;
                    }
                    "pPr" => in_ppr = false,
                    "r" if !in_table_cell => {
                        if in_run && !current_run.text.is_empty() {
                            current_paragraph.runs.push(std::mem::replace(
                                &mut current_run,
                                TextRun {
                                    text: String::new(),
                                    bold: false,
                                    italic: false,
                                },
                            ));
                        }
                        in_run = false;
                        in_rpr = false;
                    }
                    "rPr" => in_rpr = false,
                    "t" => in_text = false,
                    "tc" => {
                        table_row.push(cell_text.trim().to_string());
                        cell_text.clear();
                        in_table_cell = false;
                    }
                    "tr" => {
                        table_rows.push(table_row.clone());
                        table_row.clear();
                        in_table_row = false;
                    }
                    "tbl" => {
                        if !table_rows.is_empty() {
                            tables.push(table_rows.clone());
                        }
                        table_rows.clear();
                        in_table = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::Conversion {
                    format: "powerpoint",
                    message: format!("Failed to parse slide XML: {e}"),
                });
            }
            _ => {}
        }
    }

    // Suppress unused variable warnings
    let _ = in_rpr;

    Ok(SlideContent { shapes, tables })
}

fn write_table(writer: &mut dyn Write, rows: &[Vec<String>]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return Ok(());
    }

    // Header
    let header = &rows[0];
    write!(writer, "|")?;
    for i in 0..col_count {
        let cell = header.get(i).map(|s| s.as_str()).unwrap_or("");
        write!(writer, " {} |", cell.replace('|', "\\|"))?;
    }
    writeln!(writer)?;

    // Separator
    write!(writer, "|")?;
    for _ in 0..col_count {
        write!(writer, "---|")?;
    }
    writeln!(writer)?;

    // Data
    for row in rows.iter().skip(1) {
        write!(writer, "|")?;
        for i in 0..col_count {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            write!(writer, " {} |", cell.replace('|', "\\|"))?;
        }
        writeln!(writer)?;
    }

    Ok(())
}

fn read_entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String> {
    let mut file = archive.by_name(name).map_err(|e| Error::Conversion {
        format: "powerpoint",
        message: format!("Entry not found: {name}: {e}"),
    })?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn local_name(name: &[u8]) -> String {
    let s = std::str::from_utf8(name).unwrap_or("");
    if let Some(pos) = s.rfind(':') {
        s[pos + 1..].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::Converter;
    use rstest::rstest;
    use std::io::Write;

    fn make_pptx(slides: &[(&str, &str)]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, content) in slides {
            zip.start_file(name.to_string(), options).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn slide_xml(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
       xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
       xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:cSld><p:spTree>{body}</p:spTree></p:cSld>
</p:sld>"#
        )
    }

    fn title_shape(text: &str) -> String {
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#
        )
    }

    fn body_shape(text: &str) -> String {
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#
        )
    }

    fn formatted_shape(text: &str, bold: bool, italic: bool) -> String {
        let mut attrs = Vec::new();
        if bold {
            attrs.push(r#"b="1""#);
        }
        if italic {
            attrs.push(r#"i="1""#);
        }
        let rpr = if attrs.is_empty() {
            String::new()
        } else {
            format!("<a:rPr {}/>", attrs.join(" "))
        };
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r>{rpr}<a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#
        )
    }

    fn bullet_shape(items: &[&str]) -> String {
        let paras: String = items
            .iter()
            .map(|t| {
                format!(
                    r#"<a:p><a:pPr><a:buChar char="•"/></a:pPr><a:r><a:t>{t}</a:t></a:r></a:p>"#
                )
            })
            .collect();
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
<p:txBody>{paras}</p:txBody></p:sp>"#
        )
    }

    fn table_xml(rows: &[&[&str]]) -> String {
        let rows_xml: String = rows
            .iter()
            .map(|cells| {
                let cells_xml: String = cells
                    .iter()
                    .map(|c| {
                        format!(r#"<a:tc><a:txBody><a:p><a:r><a:t>{c}</a:t></a:r></a:p></a:txBody></a:tc>"#)
                    })
                    .collect();
                format!("<a:tr>{cells_xml}</a:tr>")
            })
            .collect();
        format!(
            r#"<a:graphicFrame><a:graphic><a:graphicData>
<a:tbl>{rows_xml}</a:tbl>
</a:graphicData></a:graphic></a:graphicFrame>"#
        )
    }

    fn convert(pptx_bytes: &[u8]) -> String {
        let converter = PowerPointConverter;
        let mut output = Vec::new();
        converter.convert(pptx_bytes, &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[rstest]
    #[case::title("title", "# Hello")]
    #[case::plain("plain", "Some content")]
    #[case::bold("bold", "**important**")]
    #[case::italic("italic", "*emphasis*")]
    #[case::bold_italic("bold_italic", "***strong***")]
    fn test_text_formatting(#[case] kind: &str, #[case] expected: &str) {
        let shape = match kind {
            "title" => title_shape("Hello"),
            "plain" => body_shape("Some content"),
            "bold" => formatted_shape("important", true, false),
            "italic" => formatted_shape("emphasis", false, true),
            "bold_italic" => formatted_shape("strong", true, true),
            _ => unreachable!(),
        };
        let xml = slide_xml(&shape);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(
            output.contains(expected),
            "Expected {expected:?} in:\n{output}"
        );
    }

    #[rstest]
    fn test_bullet_list() {
        let shape = bullet_shape(&["Item A", "Item B", "Item C"]);
        let xml = slide_xml(&shape);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("- Item A"));
        assert!(output.contains("- Item B"));
        assert!(output.contains("- Item C"));
    }

    #[rstest]
    fn test_table() {
        let tbl = table_xml(&[&["Name", "Age"], &["Alice", "30"], &["Bob", "25"]]);
        let xml = slide_xml(&tbl);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("| Name | Age |"), "Missing header in:\n{output}");
        assert!(output.contains("|---|"), "Missing separator in:\n{output}");
        assert!(output.contains("| Alice | 30 |"), "Missing row in:\n{output}");
        assert!(output.contains("| Bob | 25 |"), "Missing row in:\n{output}");
    }

    #[rstest]
    fn test_multiple_slides() {
        let s1 = slide_xml(&title_shape("Slide One"));
        let s2 = slide_xml(&title_shape("Slide Two"));
        let pptx = make_pptx(&[
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/slide2.xml", &s2),
        ]);
        let output = convert(&pptx);
        assert!(output.contains("# Slide One"));
        assert!(output.contains("---"));
        assert!(output.contains("# Slide Two"));
    }

    #[rstest]
    fn test_empty_slide() {
        let xml = slide_xml("");
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("*Empty slide*"));
    }

    #[rstest]
    fn test_slide_ordering() {
        let s1 = slide_xml(&title_shape("First"));
        let s2 = slide_xml(&title_shape("Second"));
        let s3 = slide_xml(&title_shape("Third"));
        let pptx = make_pptx(&[
            ("ppt/slides/slide3.xml", &s3),
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/slide2.xml", &s2),
        ]);
        let output = convert(&pptx);
        let p1 = output.find("# First").unwrap();
        let p2 = output.find("# Second").unwrap();
        let p3 = output.find("# Third").unwrap();
        assert!(p1 < p2);
        assert!(p2 < p3);
    }

    #[rstest]
    fn test_subtitle() {
        let shapes = format!(
            "{}{}",
            title_shape("Main Title"),
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="subTitle"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>Sub Title</a:t></a:r></a:p></p:txBody></p:sp>"#
        );
        let xml = slide_xml(&shapes);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("# Main Title"));
        assert!(output.contains("## Sub Title"));
    }
}
