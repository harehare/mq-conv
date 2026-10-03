//! PDF → Markdown using a glyph-position based layout pipeline.
//!
//! `lopdf` is only used to read PDF objects and decompress streams. Fonts,
//! content streams, reading order, tables, headings and lists are
//! reconstructed here from glyph coordinates.

mod cff;
mod content;
mod font;
mod glyphnames;
mod image;
mod layout;
mod table;
mod util;
mod words;

use std::collections::HashMap;
use std::io::Write;

use lopdf::{Document, Object, ObjectId};

use crate::converter::{ConvertOptions, Converter};
use crate::error::{Error, Result};

use crate::parallel::par_map;
use content::{FontCache, PageContent, extract_page};
use layout::{Block, DocStats, edge_keys, layout_page, render_blocks};
use util::{dict_get, resolve};
use words::{Word, build_words};

const PAGE_MARKER_PREFIX: &str = "<!-- page ";
const PAGE_MARKER_SUFFIX: &str = " -->";

pub struct PdfConverter;

struct Prepared {
    number: u32,
    content: PageContent,
    words: Vec<Word>,
}

fn err(message: impl ToString) -> Error {
    Error::Conversion {
        format: "pdf",
        message: message.to_string(),
    }
}

impl Converter for PdfConverter {
    fn format_name(&self) -> &'static str {
        "pdf"
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
        let mut doc = Document::load_mem(input).map_err(err)?;
        if doc.is_encrypted() {
            doc.decrypt("")
                .map_err(|_| err("PDF is encrypted and requires a password"))?;
        }

        let pages: Vec<(u32, ObjectId)> = doc.get_pages().into_iter().collect();
        let fonts: FontCache = Default::default();

        // `--pages` limits what is rendered, not what is analysed: heading
        // levels and running headers come from statistics over the whole
        // document, so a page converts the same with or without the option.
        let selected = |number: u32| options.is_selected(number);
        options.check_pages("pdf", "page", pages.len())?;

        // Phase 1: per-page extraction (parallel, failures isolated per page).
        let prepared: Vec<Option<Prepared>> = par_map(&pages, |&(number, id)| {
            let content = extract_page(&doc, id, &fonts);
            let words = build_words(&content.glyphs, &content.links);
            Prepared {
                number,
                content,
                words,
            }
        });

        // Document-wide statistics.
        let word_sets: Vec<&[Word]> = prepared
            .iter()
            .flatten()
            .map(|p| p.words.as_slice())
            .collect();
        let (body_size, heading_sizes) = layout::compute_body_and_headings(&word_sets);
        let mut edge_texts = std::collections::HashSet::new();
        if word_sets.len() >= 3 {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for p in prepared.iter().flatten() {
                for k in edge_keys(&p.words, p.content.height) {
                    *counts.entry(k).or_default() += 1;
                }
            }
            // Running heads are often chapter-specific, so a modest absolute
            // repeat count is enough for large documents.
            let threshold = (word_sets.len() * 6 / 10).clamp(3, 6);
            edge_texts = counts
                .into_iter()
                .filter(|&(_, n)| n >= threshold)
                .map(|(k, _)| k)
                .collect();
        }
        let stats = DocStats {
            body_size,
            heading_sizes,
            edge_texts,
        };

        let wanted = || prepared.iter().flatten().filter(|p| selected(p.number));
        let any_text = wanted().any(|p| !p.words.is_empty());
        let any_image = wanted().any(|p| !p.content.images.is_empty());
        // The fallback describes the whole document. With `--pages` the caller
        // asked for specific pages, so each keeps its marker even when blank.
        if options.pages.is_none() && !any_text && !(any_image && cfg!(feature = "ocr")) {
            writeln!(
                writer,
                "*PDF contains no extractable text (may be scanned/image-based)*"
            )?;
            return Ok(());
        }

        // Phase 2: layout + rendering (parallel).
        let image_dir = options.image_dir.as_deref();
        if let Some(dir) = image_dir {
            std::fs::create_dir_all(dir)?;
        }
        let rendered: Vec<Option<String>> = par_map(&prepared, |p| match p {
            Some(p) if selected(p.number) => render_page(&doc, p, &stats, options),
            _ => String::new(),
        });

        let mut markdown = String::new();
        for (i, (page, text)) in pages.iter().zip(rendered).enumerate() {
            if !selected(page.0) {
                continue;
            }
            if !markdown.is_empty() {
                markdown.push('\n');
            }
            markdown.push_str(&format!(
                "{PAGE_MARKER_PREFIX}{}{PAGE_MARKER_SUFFIX}\n\n",
                page.0
            ));
            match text {
                Some(t) if prepared[i].is_some() => markdown.push_str(&t),
                _ => markdown.push_str("<!-- page extraction failed -->\n"),
            }
        }

        let body_title = body_leading_heading(&markdown);
        let meta = read_metadata(&doc);
        writeln!(writer, "{}", write_metadata(&meta, body_title.as_deref()))?;
        write!(writer, "{markdown}")?;
        Ok(())
    }
}

fn render_page(doc: &Document, p: &Prepared, stats: &DocStats, options: &ConvertOptions) -> String {
    let with_images = options.image_dir.is_some();
    let blocks = layout_page(&p.content, p.words.clone(), stats, with_images);

    // Image blocks are not text: a scanned page that only yielded images still
    // needs OCR, and keeps its image links next to the recognised text.
    #[cfg(feature = "ocr")]
    let ocr_text = (!blocks.iter().any(|b| !matches!(b, Block::Image(_))))
        .then(|| ocr_page(doc, p, options))
        .flatten();
    #[cfg(not(feature = "ocr"))]
    let ocr_text: Option<String> = None;

    let blocks: Vec<Block> = blocks
        .into_iter()
        .map(|b| match b {
            Block::Image(i) => {
                let placed = &p.content.images[i];
                match save_image(doc, placed.id, p.number, i, options) {
                    Some(path) => Block::Raw(format!("![image]({path})")),
                    None => Block::Raw(String::new()),
                }
            }
            other => other,
        })
        .filter(|b| !matches!(b, Block::Raw(s) if s.is_empty()))
        .collect();
    let mut out = render_blocks(&blocks);
    if let Some(text) = ocr_text {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&text);
    }
    out
}

fn save_image(
    doc: &Document,
    id: ObjectId,
    page: u32,
    index: usize,
    options: &ConvertOptions,
) -> Option<String> {
    let dir = options.image_dir.as_ref()?;
    let link_dir = options.image_link_dir.as_ref().unwrap_or(dir);
    let img = image::extract(doc, id)?;
    let name = format!("page{page}-img{}.{}", index + 1, img.ext);
    std::fs::write(dir.join(&name), &img.data).ok()?;
    Some(crate::formats::media::md_path(&link_dir.join(&name)))
}

#[cfg(feature = "ocr")]
fn ocr_page(doc: &Document, p: &Prepared, options: &ConvertOptions) -> Option<String> {
    let largest = p.content.images.iter().max_by(|a, b| {
        ((a.x1 - a.x0) * (a.y1 - a.y0)).total_cmp(&((b.x1 - b.x0) * (b.y1 - b.y0)))
    })?;
    let img = image::extract(doc, largest.id)?;
    let lang = options.ocr_lang.clone().unwrap_or_else(|| "eng".into());
    let mut lt = leptess::LepTess::new(None, &lang).ok()?;
    lt.set_image_from_mem(&img.data).ok()?;
    let text = lt.get_utf8_text().ok()?;
    let text = text
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    (!text.is_empty()).then(|| format!("{text}\n"))
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Metadata {
    title: Option<String>,
    author: Option<String>,
    subject: Option<String>,
    creator: Option<String>,
    producer: Option<String>,
    created: Option<String>,
    modified: Option<String>,
}

fn decode_text_string(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    // PDFDocEncoding is close enough to Latin-1 for metadata.
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// `D:YYYYMMDDHHmmSS…` → `YYYY-MM-DDTHH:MM:SS` plus the zone designator the
/// PDF gave: `Z` for UTC, `±HH:MM` for an offset (kept, not applied). A date
/// without a zone has an unknown relationship to UT, so none is emitted.
fn format_pdf_date(s: &str) -> String {
    let d = s.trim().strip_prefix("D:").unwrap_or(s.trim());
    let digits: String = d.chars().take_while(|c| c.is_ascii_digit()).collect();
    let part = |a: usize, b: usize, default: &str| digits.get(a..b).unwrap_or(default).to_string();
    if digits.len() < 4 {
        return s.to_string();
    }
    format!(
        "{}-{}-{}T{}:{}:{}{}",
        part(0, 4, "0000"),
        part(4, 6, "01"),
        part(6, 8, "01"),
        part(8, 10, "00"),
        part(10, 12, "00"),
        part(12, 14, "00"),
        pdf_date_zone(&d[digits.len()..])
    )
}

/// Zone suffix of a PDF date: `Z`, `+HH'mm'`, `-HH'mm'`, `+HH`, `+HH:mm`…
fn pdf_date_zone(rest: &str) -> String {
    let mut chars = rest.chars();
    let sign = match chars.next() {
        Some('Z' | 'z') => return "Z".to_string(),
        Some(c @ ('+' | '-')) => c,
        _ => return String::new(),
    };
    let zone: String = chars.filter(char::is_ascii_digit).collect();
    let (Some(hh), mm) = (zone.get(0..2), zone.get(2..4).unwrap_or("00")) else {
        return String::new();
    };
    format!("{sign}{hh}:{mm}")
}

fn read_metadata(doc: &Document) -> Metadata {
    let mut meta = Metadata::default();
    let Some(info) = doc
        .trailer
        .get(b"Info")
        .ok()
        .map(|o| resolve(doc, o))
        .and_then(|o| o.as_dict().ok())
    else {
        return meta;
    };
    let get = |key: &[u8]| -> Option<String> {
        match dict_get(doc, info, key) {
            Some(Object::String(s, _)) => {
                let t = decode_text_string(s).trim().to_string();
                (!t.is_empty()).then_some(t)
            }
            _ => None,
        }
    };
    meta.title = get(b"Title");
    meta.author = get(b"Author");
    meta.subject = get(b"Subject");
    meta.creator = get(b"Creator");
    meta.producer = get(b"Producer");
    meta.created = get(b"CreationDate").map(|d| format_pdf_date(&d));
    meta.modified = get(b"ModDate").map(|d| format_pdf_date(&d));
    meta
}

fn is_page_marker(line: &str) -> bool {
    line.starts_with(PAGE_MARKER_PREFIX) && line.ends_with(PAGE_MARKER_SUFFIX)
}

fn body_leading_heading(markdown: &str) -> Option<String> {
    for line in markdown.lines() {
        let t = line.trim();
        if t.is_empty() || is_page_marker(t) {
            continue;
        }
        return t.strip_prefix("# ").map(|s| s.trim().to_string());
    }
    None
}

fn write_metadata(meta: &Metadata, body_title: Option<&str>) -> String {
    let mut out = String::new();
    let meta_title = meta.title.as_deref();

    match (meta_title, body_title) {
        (_, Some(_)) => {}
        (Some(title), None) => out.push_str(&format!("# {title}\n\n")),
        (None, None) => out.push_str("# PDF Document\n\n"),
    }

    let mut fields: Vec<(&str, String)> = Vec::new();
    if let (Some(title), Some(body)) = (meta_title, body_title)
        && !title.trim().eq_ignore_ascii_case(body.trim())
    {
        fields.push(("Title", title.to_string()));
    }
    for (label, value) in [
        ("Author", &meta.author),
        ("Subject", &meta.subject),
        ("Creator", &meta.creator),
        ("Producer", &meta.producer),
        ("Created", &meta.created),
        ("Modified", &meta.modified),
    ] {
        if let Some(v) = value {
            fields.push((label, v.clone()));
        }
    }
    for (label, value) in &fields {
        out.push_str(&format!("- **{label}**: {value}\n"));
    }
    if !fields.is_empty() {
        out.push('\n');
    }
    out.push_str("---\n");
    out
}

#[cfg(test)]
mod tests;
