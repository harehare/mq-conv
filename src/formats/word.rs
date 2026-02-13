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

        let document_xml = read_entry(&mut archive, "word/document.xml")?;
        let paragraphs = parse_document(&document_xml)?;

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

fn parse_document(xml: &str) -> Result<Vec<Paragraph>> {
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
                                current_style = Some(
                                    String::from_utf8_lossy(&attr.value).to_string(),
                                );
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
                        current_text.push_str(&formatted);
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
