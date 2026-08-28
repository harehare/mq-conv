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
        let numbering = read_entry(&mut archive, "word/numbering.xml")
            .map(|xml| parse_numbering(&xml))
            .unwrap_or_default();
        let document_xml = read_entry(&mut archive, "word/document.xml")?;
        let paragraphs = parse_document(&document_xml, &rels, &numbering)?;

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
                Paragraph::ListItem { text, level, marker } => {
                    let indent = "  ".repeat(*level as usize);
                    match marker {
                        ListMarker::Bullet => writeln!(writer, "{indent}- {text}")?,
                        ListMarker::Ordered(n) => writeln!(writer, "{indent}{n}. {text}")?,
                    }
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
    ListItem {
        text: String,
        level: u32,
        marker: ListMarker,
    },
    BlockQuote(String),
    Table(Vec<Vec<String>>),
}

enum ListMarker {
    Bullet,
    Ordered(u32),
}

fn parse_document(
    xml: &str,
    rels: &HashMap<String, String>,
    numbering: &HashMap<(String, u32), bool>,
) -> Result<Vec<Paragraph>> {
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
    let mut current_ilvl: u32 = 0;
    let mut current_num_id: Option<String> = None;
    let mut list_counters: HashMap<(String, u32), u32> = HashMap::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row: Vec<String> = Vec::new();
    let mut cell_text = String::new();
    let mut hyperlink_target: Option<String> = None;
    let mut hyperlink_buf = String::new();
    let mut in_hyperlink = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "p" => {
                        if in_table_cell && !cell_text.is_empty() {
                            cell_text.push_str("<br>");
                        }
                        in_paragraph = true;
                        current_text.clear();
                        current_style = None;
                        is_bold = false;
                        is_italic = false;
                        is_list_item = false;
                        current_ilvl = 0;
                        current_num_id = None;
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
                    "numPr" => is_list_item = true,
                    "ilvl" => {
                        is_list_item = true;
                        current_ilvl = attr_value(&e, "val")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                    }
                    "numId" => current_num_id = attr_value(&e, "val"),
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
                        if !in_table_cell && in_paragraph {
                            let list_item = |list_counters: &mut HashMap<(String, u32), u32>| {
                                let ordered = current_num_id.as_deref().is_some_and(|id| {
                                    numbering
                                        .get(&(id.to_string(), current_ilvl))
                                        .copied()
                                        .unwrap_or(false)
                                });
                                let marker = if ordered {
                                    let key =
                                        (current_num_id.clone().unwrap_or_default(), current_ilvl);
                                    let n = list_counters.entry(key).or_insert(0);
                                    *n += 1;
                                    ListMarker::Ordered(*n)
                                } else {
                                    ListMarker::Bullet
                                };
                                Paragraph::ListItem {
                                    text: current_text.clone(),
                                    level: current_ilvl,
                                    marker,
                                }
                            };
                            let para = if let Some(ref style) = current_style {
                                if let Some(level) = heading_level(style) {
                                    Paragraph::Heading(level, current_text.clone())
                                } else if is_blockquote(style) {
                                    Paragraph::BlockQuote(current_text.clone())
                                } else if is_list_item {
                                    list_item(&mut list_counters)
                                } else {
                                    Paragraph::Text(current_text.clone())
                                }
                            } else if is_list_item {
                                list_item(&mut list_counters)
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

fn attr_value(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    let prefixed = format!("w:{name}");
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name.as_bytes() || a.key.as_ref() == prefixed.as_bytes())
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
}

fn parse_numbering(xml: &str) -> HashMap<(String, u32), bool> {
    let mut reader = Reader::from_str(xml);
    let mut abstract_fmt: HashMap<(String, u32), String> = HashMap::new();
    let mut num_to_abstract: HashMap<String, String> = HashMap::new();

    let mut current_abstract_id: Option<String> = None;
    let mut current_num_id: Option<String> = None;
    let mut current_lvl: Option<u32> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "abstractNum" => current_abstract_id = attr_value(&e, "abstractNumId"),
                    "num" => current_num_id = attr_value(&e, "numId"),
                    "lvl" => {
                        current_lvl = attr_value(&e, "ilvl").and_then(|v| v.parse().ok());
                    }
                    "abstractNumId" => {
                        if let (Some(num_id), Some(val)) = (&current_num_id, attr_value(&e, "val"))
                        {
                            num_to_abstract.insert(num_id.clone(), val);
                        }
                    }
                    "numFmt" => {
                        if let (Some(abs_id), Some(lvl), Some(val)) =
                            (&current_abstract_id, current_lvl, attr_value(&e, "val"))
                        {
                            abstract_fmt.insert((abs_id.clone(), lvl), val);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => match local_name(e.name().as_ref()).as_str() {
                "abstractNum" => current_abstract_id = None,
                "num" => current_num_id = None,
                "lvl" => current_lvl = None,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let mut result = HashMap::new();
    for (num_id, abs_id) in &num_to_abstract {
        for ((a, lvl), fmt) in &abstract_fmt {
            if a == abs_id {
                result.insert((num_id.clone(), *lvl), fmt != "bullet" && fmt != "none");
            }
        }
    }
    result
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
        write!(writer, " {} |", escape_cell(cell))?;
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
            write!(writer, " {} |", escape_cell(cell))?;
        }
        writeln!(writer)?;
    }

    Ok(())
}

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
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

    fn make_docx_with_numbering(document_xml: &str, numbering_xml: &str) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        zip.start_file("word/numbering.xml", options).unwrap();
        zip.write_all(numbering_xml.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
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

    const NUMBERING_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="1"><w:numFmt w:val="lowerLetter"/></w:lvl>
  </w:abstractNum>
  <w:abstractNum w:abstractNumId="1">
    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;

    fn numbered_item(text: &str, num_id: u32, ilvl: u32) -> String {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{ilvl}"/><w:numId w:val="{num_id}"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
        )
    }

    #[test]
    fn test_ordered_list_numbered_sequentially() {
        let body = format!(
            "{}{}{}",
            numbered_item("First", 1, 0),
            numbered_item("Second", 1, 0),
            numbered_item("Third", 1, 0)
        );
        let docx = make_docx_with_numbering(&doc_xml(&body), NUMBERING_XML);
        let out = convert(&docx);
        assert!(out.contains("1. First"), "{out}");
        assert!(out.contains("2. Second"), "{out}");
        assert!(out.contains("3. Third"), "{out}");
    }

    #[test]
    fn test_bullet_numbering_format_stays_a_bullet() {
        let body = numbered_item("Item", 2, 0);
        let docx = make_docx_with_numbering(&doc_xml(&body), NUMBERING_XML);
        let out = convert(&docx);
        assert!(out.contains("- Item"), "{out}");
        assert!(!out.contains("1. Item"), "{out}");
    }

    #[test]
    fn test_nested_list_item_is_indented() {
        let body = format!("{}{}", numbered_item("Top", 1, 0), numbered_item("Sub", 1, 1));
        let docx = make_docx_with_numbering(&doc_xml(&body), NUMBERING_XML);
        let out = convert(&docx);
        assert!(out.contains("1. Top"), "{out}");
        assert!(out.contains("  1. Sub"), "{out}");
    }

    #[test]
    fn test_multi_paragraph_table_cell_joined_with_br() {
        let body = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Line one</w:t></w:r></w:p><w:p><w:r><w:t>Line two</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
        let docx = make_docx(&doc_xml(body), None);
        let out = convert(&docx);
        assert!(out.contains("Line one<br>Line two"), "{out}");
    }
}
