use std::io::Write;

use crate::converter::{ConvertOptions, Converter};
use crate::document::{Block, Document};
use crate::error::{Error, Result};
use crate::formats::media::MediaWriter;

/// Marker that stands in for `recindex` image references while the HTML is
/// converted, so they can be resolved (or dropped) afterwards.
const IMAGE_MARKER: &str = "mobi-recindex-";

pub struct MobiConverter;

impl Converter for MobiConverter {
    fn format_name(&self) -> &'static str {
        "mobi"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        self.convert_with(input, writer, &ConvertOptions::default())
    }

    fn convert_with(
        &self,
        input: &[u8],
        writer: &mut dyn Write,
        options: &ConvertOptions,
    ) -> Result<()> {
        let mut media = MediaWriter::from_options(options);
        let book = mobi::Mobi::new(input.to_vec()).map_err(|e| Error::Conversion {
            format: "mobi",
            message: e.to_string(),
        })?;

        if !matches!(book.encryption(), mobi::headers::Encryption::No) {
            return Err(Error::Conversion {
                format: "mobi",
                message: "the book is DRM-protected and cannot be converted".into(),
            });
        }

        let html = book.content_as_string_lossy();
        let html = rewrite_images(&html);

        // Chapters are the page-break separated parts that yield text,
        // numbered in reading order. Numbering comes from the text alone, so a
        // chapter keeps its number under any option.
        let chapters: Vec<String> = split_chapters(&html)
            .into_iter()
            .map(html_to_markdown)
            .filter(|text| !text.trim().is_empty())
            .collect();
        options.check_pages("mobi", "chapter", chapters.len())?;

        let title = book.title();
        let mut doc = Document::new();
        doc.push(Block::heading(
            1,
            if title.trim().is_empty() {
                "MOBI"
            } else {
                &title
            },
        ));
        let language = format!("{:?}", book.language());
        for (label, value) in [
            ("Author", book.author()),
            ("Language", Some(language).filter(|l| l != "Neutral")),
            ("Publisher", book.publisher()),
            ("Date", book.publish_date()),
        ] {
            if let Some(value) = value.filter(|v| !v.trim().is_empty()) {
                doc.push(Block::field(label, &value));
            }
        }
        if let Some(description) = book.description().filter(|d| !d.trim().is_empty()) {
            // The description is HTML in most books.
            let description = html_to_markdown(&description);
            doc.push(Block::Quote(vec![Block::paragraph(description.trim())]));
        }

        let images = book.image_records();
        for (idx, text) in chapters.iter().enumerate() {
            // Unselected chapters are skipped before any image is extracted.
            if !options.is_selected(idx as u32 + 1) {
                continue;
            }
            let text = resolve_images(text, &images, media.as_mut());
            doc.push(Block::Rule);
            doc.push(Block::Markdown(text));
        }

        doc.write_markdown(writer)
    }
}

/// Split the book HTML at its `<mbp:pagebreak/>` markers.
fn split_chapters(html: &str) -> Vec<&str> {
    const MARKER: &str = "<mbp:pagebreak";
    let lower = html.to_ascii_lowercase();
    let mut parts = Vec::new();
    let mut start = 0;
    while let Some(pos) = lower[start..].find(MARKER) {
        let at = start + pos;
        parts.push(&html[start..at]);
        // Skip the tag itself.
        start = lower[at..].find('>').map_or(html.len(), |end| at + end + 1);
    }
    parts.push(&html[start..]);
    parts
}

/// Point `<img recindex="N">` at a placeholder source the HTML converter keeps
/// intact, and drop the MOBI-only `mbp:` wrapper tags it does not know.
fn rewrite_images(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut pos = 0;
    while let Some(found) = lower[pos..].find("<img") {
        let start = pos + found;
        let Some(end) = lower[start..].find('>').map(|e| start + e + 1) else {
            break;
        };
        out.push_str(&html[pos..start]);
        let tag = &html[start..end];
        match attribute(tag, "recindex").and_then(|v| v.trim().parse::<usize>().ok()) {
            Some(n) => out.push_str(&format!("<img src=\"{IMAGE_MARKER}{n}\" alt=\"\">")),
            None => out.push_str(tag),
        }
        pos = end;
    }
    out.push_str(&html[pos..]);
    out
}

/// Value of `name="..."` (or single-quoted / bare) inside an HTML tag.
fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(name) {
        let at = from + found;
        from = at + name.len();
        let boundary = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric();
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=').filter(|_| boundary) else {
            continue;
        };
        let rest = rest.trim_start();
        return Some(match rest.chars().next()? {
            q @ ('"' | '\'') => {
                let body = &rest[1..];
                &body[..body.find(q)?]
            }
            _ => {
                &rest[..rest
                    .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                    .unwrap_or(rest.len())]
            }
        });
    }
    None
}

/// Replace `![alt](mobi-recindex-N)` with a link to the extracted image, or
/// remove the image when no extraction was requested.
fn resolve_images(
    markdown: &str,
    images: &[mobi::record::RawRecord<'_>],
    mut media: Option<&mut MediaWriter>,
) -> String {
    let pattern = format!("]({IMAGE_MARKER}");
    let mut out = String::with_capacity(markdown.len());
    let mut rest = markdown;
    while let Some(close) = rest.find(&pattern) {
        // The `![alt` opener belongs to this image.
        let Some(open) = rest[..close].rfind("![") else {
            out.push_str(&rest[..close + pattern.len()]);
            rest = &rest[close + pattern.len()..];
            continue;
        };
        let digits_at = close + pattern.len();
        let Some(len) = rest[digits_at..].find(')') else {
            break;
        };
        out.push_str(&rest[..open]);
        let index: Option<usize> = rest[digits_at..digits_at + len].parse().ok();
        let record = index
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| images.get(n));
        if let (Some(media), Some(record), Some(n)) = (media.as_deref_mut(), record, index) {
            let name = format!("mobi-image-{n:05}.{}", image_extension(record.content));
            if let Some(path) = media.save(&name, record.content) {
                out.push_str(&format!("![]({path})"));
            }
        }
        rest = &rest[digits_at + len + 1..];
    }
    out.push_str(rest);
    out
}

/// File extension for an image record, from its magic bytes.
fn image_extension(data: &[u8]) -> &'static str {
    if data.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        "png"
    } else if data.starts_with(b"GIF8") {
        "gif"
    } else if data.starts_with(b"BM") {
        "bmp"
    } else if data.len() >= 12 && &data[8..12] == b"WEBP" {
        "webp"
    } else {
        "jpg"
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_at_page_breaks() {
        let parts = split_chapters("<p>a</p><mbp:pagebreak/><p>b</p><MBP:PAGEBREAK ><p>c</p>");
        assert_eq!(parts, vec!["<p>a</p>", "<p>b</p>", "<p>c</p>"]);
    }

    #[test]
    fn rewrites_recindex_images() {
        let html = r#"<p>x</p><img align="baseline" recindex="00012"/><img src="a.png">"#;
        let out = rewrite_images(html);
        assert!(out.contains(r#"<img src="mobi-recindex-12" alt="">"#));
        assert!(out.contains(r#"<img src="a.png">"#));
    }

    #[test]
    fn reads_attributes() {
        assert_eq!(attribute(r#"<img recindex="7">"#, "recindex"), Some("7"));
        assert_eq!(attribute("<img recindex='7' >", "recindex"), Some("7"));
        assert_eq!(attribute("<img recindex=7>", "recindex"), Some("7"));
        assert_eq!(attribute("<img src=a>", "recindex"), None);
    }

    #[test]
    fn images_are_dropped_without_extraction() {
        let md = "before ![](mobi-recindex-1) after";
        assert_eq!(resolve_images(md, &[], None), "before  after");
    }

    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, b'I', b'H', b'D', b'R',
    ];

    /// An uncompressed single-text-record MOBI with EXTH metadata and one image.
    fn make_mobi(html: &str) -> Vec<u8> {
        const NONE: u32 = 0xFFFF_FFFF;
        let u32s = |v: u32| v.to_be_bytes().to_vec();
        let title = b"Test Book";
        let mut exth_body = Vec::new();
        for (kind, value) in [(100u32, &b"Jane Author"[..]), (101, b"Pub Co")] {
            exth_body.extend(u32s(kind));
            exth_body.extend(u32s(8 + value.len() as u32));
            exth_body.extend(value);
        }
        let mut exth = b"EXTH".to_vec();
        exth.extend(u32s(12 + exth_body.len() as u32));
        exth.extend(u32s(2));
        exth.extend(exth_body);
        while !exth.len().is_multiple_of(4) {
            exth.push(0);
        }

        // Record 0 = PalmDOC header + MOBI header + EXTH + title; record 1 is
        // the text; record 2 the image. Real books put `first_non_book_index`
        // two past the last text record.
        let mut mobi_header = b"MOBI".to_vec();
        mobi_header.extend(u32s(232));
        for field in [2, 65001, 1, 6, NONE, NONE, NONE, NONE] {
            mobi_header.extend(u32s(field));
        }
        mobi_header.extend(std::iter::repeat_n(0xFF, 24));
        mobi_header.extend(u32s(3)); // first_non_book_index
        mobi_header.extend(u32s(16 + 232 + exth.len() as u32)); // name offset
        mobi_header.extend(u32s(title.len() as u32));
        mobi_header.extend([0, 0, 0, 9]); // unused, locale, language
        mobi_header.extend(u32s(0));
        mobi_header.extend(u32s(0));
        mobi_header.extend(u32s(6));
        mobi_header.extend(u32s(2)); // first_image_index
        for field in [0, 0, 0, 0, 0x40] {
            mobi_header.extend(u32s(field));
        }
        mobi_header.extend([0; 32]);
        for field in [0, NONE, 0, 0, 0] {
            mobi_header.extend(u32s(field));
        }
        mobi_header.extend([0; 8]);
        mobi_header.extend([0, 1, 0, 1]); // first / last content record
        for field in [0, NONE, 0, NONE, 0] {
            mobi_header.extend(u32s(field));
        }
        mobi_header.extend([0; 8]);
        for field in [0, NONE, 0, 0, 0, NONE] {
            mobi_header.extend(u32s(field));
        }
        mobi_header.resize(232, 0);

        let mut record0 = vec![0, 1, 0, 0]; // no compression
        record0.extend(u32s(html.len() as u32));
        record0.extend([0, 1, 0x10, 0, 0, 0, 0, 0]);
        record0.extend(mobi_header);
        record0.extend(exth);
        record0.extend(title);
        record0.extend([0, 0]);

        let records = [record0, html.as_bytes().to_vec(), PNG.to_vec()];
        let mut out = b"Test_Book".to_vec();
        out.resize(60, 0);
        out.extend(b"BOOKMOBI");
        out.extend(u32s(0));
        out.extend(u32s(0));
        out.extend((records.len() as u16).to_be_bytes());
        let mut offset = 78 + 8 * records.len() as u32 + 2;
        for record in &records {
            out.extend(u32s(offset));
            out.extend([0; 4]);
            offset += record.len() as u32;
        }
        out.extend([0, 0]);
        records.iter().for_each(|r| out.extend(r));
        out
    }

    const BOOK: &str = "<html><body><h1>One</h1><p>Hello <b>world</b>.</p>\
        <img recindex=\"00001\"/><mbp:pagebreak/><h1>Two</h1><p>Second.</p></body></html>";

    fn convert(options: &ConvertOptions) -> Result<String> {
        let mut out = Vec::new();
        MobiConverter.convert_with(&make_mobi(BOOK), &mut out, options)?;
        Ok(String::from_utf8(out).unwrap())
    }

    #[test]
    fn converts_metadata_and_chapters() {
        let md = convert(&ConvertOptions::default()).unwrap();
        assert!(md.starts_with("# Test Book"));
        assert!(md.contains("**Author**: Jane Author"));
        assert!(md.contains("**Publisher**: Pub Co"));
        assert!(md.contains("# One") && md.contains("Hello **world**."));
        assert!(md.contains("# Two"));
        assert!(!md.contains("!["), "images are dropped without extraction");
    }

    #[test]
    fn pages_select_chapters() {
        let options = ConvertOptions {
            pages: Some("2".parse().unwrap()),
            ..Default::default()
        };
        let md = convert(&options).unwrap();
        assert!(md.contains("# Two") && !md.contains("# One"));

        let options = ConvertOptions {
            pages: Some("9".parse().unwrap()),
            ..Default::default()
        };
        assert!(convert(&options).is_err());
    }

    #[test]
    fn extracts_images() {
        let dir = std::env::temp_dir().join(format!("mq-conv-mobi-{}", std::process::id()));
        let options = ConvertOptions {
            image_dir: Some(dir.clone()),
            ..Default::default()
        };
        let md = convert(&options).unwrap();
        assert!(md.contains("mobi-image-00001.png"), "{md}");
        assert_eq!(
            std::fs::read(dir.join("mobi-image-00001.png")).unwrap(),
            PNG
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_non_mobi_input() {
        let mut out = Vec::new();
        assert!(
            MobiConverter
                .convert(b"not a mobi file at all", &mut out)
                .is_err()
        );
    }
}
