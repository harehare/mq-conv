use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct WordConverter;

impl Converter for WordConverter {
    fn format_name(&self) -> &'static str {
        "word"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let cursor = Cursor::new(input);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| Error::Conversion {
            format: "word",
            message: e.to_string(),
        })?;

        let rels = read_entry(&mut archive, "word/_rels/document.xml.rels")
            .map(|xml| parse_relationships(&xml))
            .unwrap_or_default();
        let document_xml = read_entry(&mut archive, "word/document.xml")?;
        let paragraphs = parse_document(&document_xml, &rels)?;

        let mut first = true;
        for para in &paragraphs {
            match para {
                Paragraph::Heading(level, text) => {
                    if !first {
                        writeln!(writer)?;
                    }
                    let hashes = "#".repeat(*level as usize);
                    writeln!(writer, "{hashes} {text}")?;
                }
                Paragraph::Text(text) => {
                    if !text.is_empty() {
                        if !first {
                            writeln!(writer)?;
                        }
                        writeln!(writer, "{text}")?;
                    }
                }
                Paragraph::ListItem(text) => {
                    writeln!(writer, "- {text}")?;
                }
                Paragraph::BlockQuote(text) => {
                    if !first {
                        writeln!(writer)?;
                    }
                    writeln!(writer, "> {text}")?;
                }
                Paragraph::Table(rows) => {
                    if !first {
                        writeln!(writer)?;
                    }
                    write_table(writer, rows)?;
                }
            }
            first = false;
        }

        Ok(())
    }
}

enum Paragraph {
    Heading(u8, String),
    Text(String),
    ListItem(String),
    BlockQuote(String),
    Table(Vec<Vec<String>>),
}

fn parse_document(xml: &str, rels: &HashMap<String, String>) -> Result<Vec<Paragraph>> {
    let mut paragraphs = Vec::new();
    let mut reader = Reader::from_str(xml);

    let mut in_paragraph = false;
    let mut in_run = false;
    let mut in_table = false;
    let mut in_table_row = false;
    let mut in_table_cell = false;
    let mut current_text = String::new();
    let mut current_style: Option<String> = None;
    let mut is_bold = false;
    let mut is_italic = false;
    let mut is_list_item = false;
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row: Vec<String> = Vec::new();
    let mut cell_text = String::new();
    // Runs inside a `<w:hyperlink>` are buffered and wrapped in `[...](url)`
    // once the hyperlink closes, since a link can span multiple formatted runs.
    let mut hyperlink_target: Option<String> = None;
    let mut hyperlink_buf = String::new();
    let mut in_hyperlink = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "p" => {
                        in_paragraph = true;
                        current_text.clear();
                        current_style = None;
                        is_bold = false;
                        is_italic = false;
                        is_list_item = false;
                    }
                    "r" => in_run = true,
                    "hyperlink" => {
                        in_hyperlink = true;
                        hyperlink_buf.clear();
                        hyperlink_target = e
                            .attributes()
                            .flatten()
                            .find(|attr| attr.key.as_ref() == b"r:id" || attr.key.as_ref() == b"id")
                            .and_then(|attr| {
                                rels.get(&String::from_utf8_lossy(&attr.value).to_string())
                                    .cloned()
                            });
                    }
                    "tbl" => {
                        in_table = true;
                        table_rows.clear();
                    }
                    "tr" => {
                        in_table_row = true;
                        table_row.clear();
                    }
                    "tc" => {
                        in_table_cell = true;
                        cell_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "pStyle" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"w:val" || attr.key.as_ref() == b"val" {
                                current_style =
                                    Some(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    }
                    "b" => is_bold = true,
                    "i" => is_italic = true,
                    "numPr" | "ilvl" => is_list_item = true,
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                if in_run || in_table_cell {
                    let text = e.decode().unwrap_or_default().to_string();
                    if in_table_cell {
                        cell_text.push_str(&text);
                    } else if in_paragraph {
                        let formatted = format_run_text(&text, is_bold, is_italic);
                        if in_hyperlink {
                            hyperlink_buf.push_str(&formatted);
                        } else {
                            current_text.push_str(&formatted);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "p" => {
                        if in_table_cell {
                            if !cell_text.is_empty() {
                                // cell text accumulated separately
                            }
                        } else if in_paragraph {
                            let para = if let Some(ref style) = current_style {
                                if let Some(level) = heading_level(style) {
                                    Paragraph::Heading(level, current_text.clone())
                                } else if is_blockquote(style) {
                                    Paragraph::BlockQuote(current_text.clone())
                                } else if is_list_item {
                                    Paragraph::ListItem(current_text.clone())
                                } else {
                                    Paragraph::Text(current_text.clone())
                                }
                            } else if is_list_item {
                                Paragraph::ListItem(current_text.clone())
                            } else {
                                Paragraph::Text(current_text.clone())
                            };
                            paragraphs.push(para);
                        }
                        in_paragraph = false;
                    }
                    "r" => {
                        in_run = false;
                        is_bold = false;
                        is_italic = false;
                    }
                    "hyperlink" => {
                        if in_hyperlink && !in_table_cell {
                            match hyperlink_target.take() {
                                Some(target) => {
                                    current_text.push_str(&format!("[{hyperlink_buf}]({target})"))
                                }
                                None => current_text.push_str(&hyperlink_buf),
                            }
                        }
                        in_hyperlink = false;
                        hyperlink_buf.clear();
                        hyperlink_target = None;
                    }
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
                            paragraphs.push(Paragraph::Table(table_rows.clone()));
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
                    format: "word",
                    message: format!("Failed to parse document.xml: {e}"),
                });
            }
            _ => {}
        }
    }

    // Suppress unused variable warnings
    let _ = in_table;
    let _ = in_table_row;

    Ok(paragraphs)
}

fn parse_relationships(xml: &str) -> HashMap<String, String> {
    let mut rels = HashMap::new();
    let mut reader = Reader::from_str(xml);

    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if local_name(e.name().as_ref()) == "Relationship" =>
            {
                let mut id = None;
                let mut target = None;
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"Id" => id = Some(String::from_utf8_lossy(&attr.value).to_string()),
                        b"Target" => {
                            target = Some(String::from_utf8_lossy(&attr.value).to_string())
                        }
                        _ => {}
                    }
                }
                if let (Some(id), Some(target)) = (id, target) {
                    rels.insert(id, target);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    rels
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

fn is_blockquote(style: &str) -> bool {
    let lower = style.to_ascii_lowercase();
    lower == "quote" || lower == "intensequote" || lower == "blockquote"
}

fn heading_level(style: &str) -> Option<u8> {
    let lower = style.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("heading") {
        rest.trim()
            .parse::<u8>()
            .ok()
            .filter(|&n| (1..=6).contains(&n))
    } else if let Some(rest) = lower.strip_prefix("titre") {
        rest.trim()
            .parse::<u8>()
            .ok()
            .filter(|&n| (1..=6).contains(&n))
    } else {
        None
    }
}

fn read_entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String> {
    let mut file = archive.by_name(name).map_err(|e| Error::Conversion {
        format: "word",
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

    fn make_docx(document_xml: &str, rels_xml: Option<&str>) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        if let Some(rels) = rels_xml {
            zip.start_file("word/_rels/document.xml.rels", options)
                .unwrap();
            zip.write_all(rels.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn doc_xml(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>{body}</w:body>
</w:document>"#
        )
    }

    fn convert(docx_bytes: &[u8]) -> String {
        let mut out = Vec::new();
        WordConverter.convert(docx_bytes, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_hyperlink_resolved_from_relationships() {
        let body =
            r#"<w:p><w:hyperlink r:id="rId1"><w:r><w:t>mq-conv</w:t></w:r></w:hyperlink></w:p>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://github.com/harehare/mq-conv" TargetMode="External"/>
</Relationships>"#;
        let docx = make_docx(&doc_xml(body), Some(rels));
        let out = convert(&docx);
        assert!(
            out.contains("[mq-conv](https://github.com/harehare/mq-conv)"),
            "expected markdown link in:\n{out}"
        );
    }

    #[test]
    fn test_hyperlink_without_relationships_falls_back_to_text() {
        let body =
            r#"<w:p><w:hyperlink r:id="rId1"><w:r><w:t>plain text</w:t></w:r></w:hyperlink></w:p>"#;
        let docx = make_docx(&doc_xml(body), None);
        let out = convert(&docx);
        assert!(out.contains("plain text"), "missing text in:\n{out}");
        assert!(
            !out.contains('['),
            "should not emit markdown link syntax:\n{out}"
        );
    }

    #[test]
    fn test_hyperlink_with_bold_run() {
        let body = r#"<w:p><w:hyperlink r:id="rId1"><w:r><w:rPr><w:b/></w:rPr><w:t>bold link</w:t></w:r></w:hyperlink></w:p>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Target="https://example.com"/>
</Relationships>"#;
        let docx = make_docx(&doc_xml(body), Some(rels));
        let out = convert(&docx);
        assert!(
            out.contains("[**bold link**](https://example.com)"),
            "expected formatted markdown link in:\n{out}"
        );
    }

    #[test]
    fn test_plain_paragraph_unaffected() {
        let body = r#"<w:p><w:r><w:t>Hello world</w:t></w:r></w:p>"#;
        let docx = make_docx(&doc_xml(body), None);
        let out = convert(&docx);
        assert!(out.contains("Hello world"));
    }
}
