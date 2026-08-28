use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct EpubConverter;

impl Converter for EpubConverter {
    fn format_name(&self) -> &'static str {
        "epub"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let cursor = Cursor::new(input);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| Error::Conversion {
            format: "epub",
            message: e.to_string(),
        })?;

        // Find the OPF file path from container.xml
        let opf_path = find_opf_path(&mut archive)?;

        // Parse the OPF for metadata and spine order
        let opf_content = read_entry(&mut archive, &opf_path)?;
        let (metadata, spine_items) = parse_opf(&opf_content)?;

        // Resolve the base directory of the OPF file
        let opf_dir = if let Some(pos) = opf_path.rfind('/') {
            &opf_path[..=pos]
        } else {
            ""
        };

        // Write metadata
        if let Some(title) = &metadata.title {
            writeln!(writer, "# {title}")?;
        } else {
            writeln!(writer, "# EPUB")?;
        }
        writeln!(writer)?;

        if let Some(author) = &metadata.author {
            writeln!(writer, "**Author**: {author}")?;
        }
        if let Some(language) = &metadata.language {
            writeln!(writer, "**Language**: {language}")?;
        }
        if let Some(publisher) = &metadata.publisher {
            writeln!(writer, "**Publisher**: {publisher}")?;
        }
        if let Some(date) = &metadata.date {
            writeln!(writer, "**Date**: {date}")?;
        }
        if let Some(description) = &metadata.description {
            writeln!(writer)?;
            writeln!(writer, "> {description}")?;
        }

        writeln!(writer)?;
        writeln!(writer, "---")?;

        // Process spine items (chapters)
        let mut chapter_num = 0;
        for item_path in &spine_items {
            let full_path = if let Some(stripped) = item_path.strip_prefix('/') {
                stripped.to_string()
            } else {
                format!("{opf_dir}{item_path}")
            };

            if let Ok(html_content) = read_entry(&mut archive, &full_path) {
                let text = html_to_markdown(&html_content);
                let text = text.trim();
                if !text.is_empty() {
                    chapter_num += 1;

                    if chapter_num > 1 {
                        writeln!(writer)?;
                        writeln!(writer, "---")?;
                    }
                    writeln!(writer)?;
                    writeln!(writer, "{text}")?;
                }
            }
        }

        Ok(())
    }
}

#[derive(Default)]
struct EpubMetadata {
    title: Option<String>,
    author: Option<String>,
    language: Option<String>,
    publisher: Option<String>,
    description: Option<String>,
    date: Option<String>,
}

fn find_opf_path(archive: &mut zip::ZipArchive<Cursor<&[u8]>>) -> Result<String> {
    let container = read_entry(archive, "META-INF/container.xml")?;
    let mut reader = Reader::from_str(&container);

    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"rootfile" => {
                for attr in e.attributes().flatten() {
                    if attr.key.as_ref() == b"full-path" {
                        return Ok(String::from_utf8_lossy(&attr.value).to_string());
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::Conversion {
                    format: "epub",
                    message: format!("Failed to parse container.xml: {e}"),
                });
            }
            _ => {}
        }
    }

    Err(Error::Conversion {
        format: "epub",
        message: "Could not find rootfile in container.xml".into(),
    })
}

fn parse_opf(content: &str) -> Result<(EpubMetadata, Vec<String>)> {
    let mut metadata = EpubMetadata::default();
    let mut manifest: Vec<(String, String)> = Vec::new(); // (id, href)
    let mut spine_ids: Vec<String> = Vec::new();

    let mut reader = Reader::from_str(content);
    let mut current_tag = String::new();
    let mut in_metadata = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "metadata" => in_metadata = true,
                    "title" | "creator" | "language" | "publisher" | "description" | "date"
                        if in_metadata =>
                    {
                        current_tag = local.clone();
                    }
                    "item" => {
                        let mut id = String::new();
                        let mut href = String::new();
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
                                b"href" => href = String::from_utf8_lossy(&attr.value).to_string(),
                                _ => {}
                            }
                        }
                        if !id.is_empty() && !href.is_empty() {
                            manifest.push((id, href));
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "item" => {
                        let mut id = String::new();
                        let mut href = String::new();
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                b"id" => id = String::from_utf8_lossy(&attr.value).to_string(),
                                b"href" => href = String::from_utf8_lossy(&attr.value).to_string(),
                                _ => {}
                            }
                        }
                        if !id.is_empty() && !href.is_empty() {
                            manifest.push((id, href));
                        }
                    }
                    "itemref" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"idref" {
                                spine_ids.push(String::from_utf8_lossy(&attr.value).to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) if !current_tag.is_empty() => {
                let text = e.decode().unwrap_or_default().to_string();
                match current_tag.as_str() {
                    "title" => metadata.title = Some(text),
                    "creator" => metadata.author = Some(text),
                    "language" => metadata.language = Some(text),
                    "publisher" => metadata.publisher = Some(text),
                    "description" => metadata.description = Some(text),
                    "date" => metadata.date = Some(text),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name().as_ref());
                if local == "metadata" {
                    in_metadata = false;
                }
                current_tag.clear();

                if local == "itemref" {
                    // Handle <itemref idref="..."></itemref> form
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::Conversion {
                    format: "epub",
                    message: format!("Failed to parse OPF: {e}"),
                });
            }
            _ => {}
        }
    }

    // Resolve spine IDs to file paths
    let spine_items: Vec<String> = spine_ids
        .iter()
        .filter_map(|id| {
            manifest
                .iter()
                .find(|(mid, _)| mid == id)
                .map(|(_, href)| href.clone())
        })
        .collect();

    Ok((metadata, spine_items))
}

fn read_entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String> {
    let mut file = archive.by_name(name).map_err(|e| Error::Conversion {
        format: "epub",
        message: format!("Entry not found: {name}: {e}"),
    })?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn html_to_markdown(html: &str) -> String {
    mq_markdown::convert_html_to_markdown(
        html,
        mq_markdown::ConversionOptions {
            extract_scripts_as_code_blocks: true,
            generate_front_matter: false,
            use_title_as_h1: false,
            base_url: None,
        },
    )
    .unwrap_or_default()
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

    fn make_epub(chapter_title: &str, chapter_body: &str) -> Vec<u8> {
        let container = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;

        let opf = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>My Book</dc:title>
    <dc:creator>Jane Doe</dc:creator>
  </metadata>
  <manifest>
    <item id="ch1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="ch1"/>
  </spine>
</package>"#;

        let chapter = format!(
            r#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>{chapter_title}</title></head>
<body>{chapter_body}</body>
</html>"#
        );

        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        for (name, content) in [
            ("META-INF/container.xml", container.to_string()),
            ("OEBPS/content.opf", opf.to_string()),
            ("OEBPS/chapter1.xhtml", chapter),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    fn convert(epub_bytes: &[u8]) -> String {
        let mut out = Vec::new();
        EpubConverter.convert(epub_bytes, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_chapter_title_not_duplicated_with_body_heading() {
        let epub = make_epub("Chapter 1", "<h1>Chapter 1</h1><p>Once upon a time.</p>");
        let out = convert(&epub);
        assert_eq!(
            out.matches("Chapter 1").count(),
            1,
            "chapter <title> and body <h1> should not both render:\n{out}"
        );
    }

    #[test]
    fn test_book_metadata_rendered_once() {
        let epub = make_epub("Intro", "<p>Hello.</p>");
        let out = convert(&epub);
        assert!(out.starts_with("# My Book"));
        assert!(out.contains("**Author**: Jane Doe"));
    }
}
