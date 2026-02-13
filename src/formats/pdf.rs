use std::io::Write;

use lopdf::Document;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct PdfConverter;

impl Converter for PdfConverter {
    fn format_name(&self) -> &'static str {
        "pdf"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let doc = Document::load_mem(input).map_err(|e| Error::Conversion {
            format: "pdf",
            message: e.to_string(),
        })?;

        write_metadata(&doc, writer)?;

        let pages = pdf_extract::extract_text_from_mem_by_pages(input).map_err(|e| {
            Error::Conversion {
                format: "pdf",
                message: e.to_string(),
            }
        })?;

        if pages.is_empty() {
            writeln!(
                writer,
                "*PDF contains no extractable text (may be scanned/image-based)*"
            )?;
            return Ok(());
        }

        let total_pages = pages.len();
        for (i, page_text) in pages.iter().enumerate() {
            writeln!(writer, "## Page {}", i + 1)?;
            writeln!(writer)?;

            let text = page_text.trim();
            if text.is_empty() {
                writeln!(writer, "*Empty page*")?;
            } else {
                write_structured_text(writer, text)?;
            }

            if i + 1 < total_pages {
                writeln!(writer)?;
                writeln!(writer, "---")?;
                writeln!(writer)?;
            }
        }

        Ok(())
    }
}

fn write_metadata(doc: &Document, writer: &mut dyn Write) -> Result<()> {
    let info = extract_info(doc);
    if info.is_empty() {
        return Ok(());
    }

    let title = info.iter().find(|(k, _)| k == "Title").map(|(_, v)| v);
    if let Some(title) = title {
        if !title.is_empty() {
            writeln!(writer, "# {title}")?;
        } else {
            writeln!(writer, "# PDF Document")?;
        }
    } else {
        writeln!(writer, "# PDF Document")?;
    }
    writeln!(writer)?;

    let mut has_meta = false;
    for (key, value) in &info {
        if key == "Title" || value.is_empty() {
            continue;
        }
        writeln!(writer, "- **{key}**: {value}")?;
        has_meta = true;
    }

    if has_meta {
        writeln!(writer)?;
    }

    writeln!(writer, "---")?;
    writeln!(writer)?;

    Ok(())
}

fn extract_info(doc: &Document) -> Vec<(String, String)> {
    let mut info = Vec::new();

    let info_dict = doc
        .trailer
        .get(b"Info")
        .ok()
        .and_then(|obj| obj.as_reference().ok())
        .and_then(|id| doc.get_dictionary(id).ok());

    let Some(dict) = info_dict else {
        return info;
    };

    let keys = [
        (b"Title".as_slice(), "Title"),
        (b"Author", "Author"),
        (b"Subject", "Subject"),
        (b"Creator", "Creator"),
        (b"Producer", "Producer"),
        (b"CreationDate", "Created"),
        (b"ModDate", "Modified"),
    ];

    for (pdf_key, label) in keys {
        if let Ok(obj) = dict.get(pdf_key) {
            let text = pdf_object_to_string(obj);
            if !text.is_empty() {
                info.push((label.to_string(), text));
            }
        }
    }

    info
}

fn pdf_object_to_string(obj: &lopdf::Object) -> String {
    match obj {
        lopdf::Object::String(bytes, _) => {
            // Try UTF-16BE (BOM: FE FF)
            if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
                let chars: Vec<u16> = bytes[2..]
                    .chunks(2)
                    .filter_map(|c| {
                        if c.len() == 2 {
                            Some(u16::from_be_bytes([c[0], c[1]]))
                        } else {
                            None
                        }
                    })
                    .collect();
                String::from_utf16_lossy(&chars)
            } else {
                String::from_utf8_lossy(bytes).to_string()
            }
        }
        _ => String::new(),
    }
}

/// Convert raw extracted text into structured Markdown paragraphs.
fn write_structured_text(writer: &mut dyn Write, text: &str) -> Result<()> {
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        if line.is_empty() {
            i += 1;
            continue;
        }

        // Detect bullet-like patterns
        if line.starts_with('•')
            || line.starts_with('●')
            || line.starts_with('○')
            || line.starts_with('-')
            || line.starts_with('–')
            || line.starts_with('*')
        {
            let content = line[line.chars().next().unwrap().len_utf8()..].trim();
            writeln!(writer, "- {content}")?;
            i += 1;
            continue;
        }

        // Detect numbered list patterns (1. or 1) or (1))
        if let Some(content) = strip_numbered_prefix(line) {
            writeln!(writer, "- {content}")?;
            i += 1;
            continue;
        }

        // Collect paragraph: consecutive non-empty lines
        let mut para = String::from(line);
        i += 1;
        while i < lines.len() {
            let next = lines[i].trim();
            if next.is_empty()
                || next.starts_with('•')
                || next.starts_with('●')
                || next.starts_with('○')
                || next.starts_with('-')
                || next.starts_with('–')
                || strip_numbered_prefix(next).is_some()
            {
                break;
            }
            para.push(' ');
            para.push_str(next);
            i += 1;
        }

        writeln!(writer, "{para}")?;
        writeln!(writer)?;
    }

    Ok(())
}

fn strip_numbered_prefix(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed.trim_start_matches(|c: char| c.is_ascii_digit());
    if rest.len() < trimmed.len() {
        if let Some(rest) = rest.strip_prefix(". ") {
            return Some(rest);
        }
        if let Some(rest) = rest.strip_prefix(") ") {
            return Some(rest);
        }
    }
    None
}
