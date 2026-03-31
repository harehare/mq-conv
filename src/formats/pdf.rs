use std::io::Write;

use lopdf::Document;
use pdf_extract::{
    ColorSpace, MediaBox, OutputDev, OutputError, Path, PathOp, Transform, output_doc,
};

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

        let mut collector = PageCollector::new();
        output_doc(&doc, &mut collector).map_err(|e| Error::Conversion {
            format: "pdf",
            message: e.to_string(),
        })?;

        if collector.pages.is_empty() {
            writeln!(
                writer,
                "*PDF contains no extractable text (may be scanned/image-based)*"
            )?;
            return Ok(());
        }

        let total_pages = collector.pages.len();
        for (i, page) in collector.pages.into_iter().enumerate() {
            writeln!(writer, "## Page {}", i + 1)?;
            writeln!(writer)?;

            if page.glyphs.is_empty() {
                writeln!(writer, "*Empty page*")?;
            } else {
                write_page_content(writer, page)?;
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

// ---------------------------------------------------------------------------
// Positional data structures
// ---------------------------------------------------------------------------

struct Glyph {
    x: f64,
    y: f64,
    advance: f64,
    ch: String,
}

struct PageData {
    glyphs: Vec<Glyph>,
    rects: Vec<(f64, f64, f64, f64)>, // (x, y, width, height)
}

struct PageCollector {
    pages: Vec<PageData>,
    current_glyphs: Vec<Glyph>,
    current_rects: Vec<(f64, f64, f64, f64)>,
}

impl PageCollector {
    fn new() -> Self {
        Self {
            pages: Vec::new(),
            current_glyphs: Vec::new(),
            current_rects: Vec::new(),
        }
    }

    fn collect_rects_from_path(&mut self, ctm: &Transform, path: &Path) {
        for op in &path.ops {
            if let PathOp::Rect(rx, ry, rw, rh) = op {
                let w = (rw * ctm.m11).abs();
                let h = (rh * ctm.m22).abs();
                // Only keep rectangles large enough to be table borders (>5pt each dimension)
                if w > 5.0 && h > 2.0 {
                    let x = ctm.m31 + rx * ctm.m11 + ry * ctm.m21;
                    let y = ctm.m32 + rx * ctm.m12 + ry * ctm.m22;
                    self.current_rects.push((x, y, w, h));
                }
            }
        }
    }
}

impl OutputDev for PageCollector {
    fn begin_page(
        &mut self,
        _page_num: u32,
        _media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> std::result::Result<(), OutputError> {
        self.current_glyphs.clear();
        self.current_rects.clear();
        Ok(())
    }

    fn end_page(&mut self) -> std::result::Result<(), OutputError> {
        self.pages.push(PageData {
            glyphs: std::mem::take(&mut self.current_glyphs),
            rects: std::mem::take(&mut self.current_rects),
        });
        Ok(())
    }

    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> std::result::Result<(), OutputError> {
        let x = trm.m31;
        let y = trm.m32;
        // Approximate advance width in page units
        let scale = (trm.m11 * trm.m11 + trm.m12 * trm.m12).sqrt();
        let advance = width.abs() * font_size.abs() * scale;
        self.current_glyphs.push(Glyph {
            x,
            y,
            advance,
            ch: char.to_string(),
        });
        Ok(())
    }

    fn begin_word(&mut self) -> std::result::Result<(), OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> std::result::Result<(), OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> std::result::Result<(), OutputError> {
        Ok(())
    }

    fn stroke(
        &mut self,
        ctm: &Transform,
        _: &ColorSpace,
        _: &[f64],
        path: &Path,
    ) -> std::result::Result<(), OutputError> {
        self.collect_rects_from_path(ctm, path);
        Ok(())
    }

    fn fill(
        &mut self,
        ctm: &Transform,
        _: &ColorSpace,
        _: &[f64],
        path: &Path,
    ) -> std::result::Result<(), OutputError> {
        self.collect_rects_from_path(ctm, path);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Word / line building
// ---------------------------------------------------------------------------

struct Word {
    x: f64,
    y: f64,
    text: String,
}

struct TextLine {
    y: f64,
    words: Vec<Word>,
}

fn build_words(mut glyphs: Vec<Glyph>) -> Vec<Word> {
    if glyphs.is_empty() {
        return Vec::new();
    }
    // Sort top-to-bottom (y descending in PDF space), then left-to-right
    glyphs.sort_by(|a, b| {
        b.y.partial_cmp(&a.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut words: Vec<Word> = Vec::new();
    let mut buf = String::new();
    let mut wx = glyphs[0].x;
    let mut wy = glyphs[0].y;
    let mut prev_x_end = glyphs[0].x + glyphs[0].advance.max(1.0);
    let mut prev_y = glyphs[0].y;

    for glyph in &glyphs {
        let y_diff = (glyph.y - prev_y).abs();
        let x_gap = glyph.x - prev_x_end;
        // New line (>3pt y diff) or significant horizontal gap = word boundary
        let new_word = y_diff > 3.0 || x_gap > 4.0;

        if new_word && !buf.trim().is_empty() {
            words.push(Word {
                x: wx,
                y: wy,
                text: buf.trim().to_string(),
            });
            buf.clear();
            wx = glyph.x;
            wy = glyph.y;
        } else if new_word {
            buf.clear();
            wx = glyph.x;
            wy = glyph.y;
        }

        if buf.is_empty() {
            wx = glyph.x;
            wy = glyph.y;
        }

        buf.push_str(&glyph.ch);
        prev_x_end = glyph.x + glyph.advance.max(1.0);
        prev_y = glyph.y;
    }

    if !buf.trim().is_empty() {
        words.push(Word {
            x: wx,
            y: wy,
            text: buf.trim().to_string(),
        });
    }

    words.retain(|w| !w.text.is_empty());
    words
}

fn build_lines(mut words: Vec<Word>) -> Vec<TextLine> {
    if words.is_empty() {
        return Vec::new();
    }
    // Sort top-to-bottom
    words.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));

    let mut lines: Vec<TextLine> = Vec::new();
    for word in words {
        if let Some(last) = lines.last_mut()
            && (word.y - last.y).abs() < 3.0
        {
            last.words.push(word);
            continue;
        }
        lines.push(TextLine {
            y: word.y,
            words: vec![word],
        });
    }

    for line in &mut lines {
        line.words
            .sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    }

    lines
}

// ---------------------------------------------------------------------------
// Table detection
// ---------------------------------------------------------------------------

/// Cluster a list of x-positions into column boundaries (within `tol` points).
fn cluster_columns(positions: &[f64], tol: f64) -> Vec<f64> {
    let mut sorted = positions.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    sorted.dedup_by(|a, b| (*a - *b).abs() < tol);
    sorted
}

/// Assign a word to the nearest column index.
fn nearest_col(x: f64, cols: &[f64]) -> usize {
    cols.iter()
        .enumerate()
        .min_by(|&(_, a), &(_, b)| {
            (x - a)
                .abs()
                .partial_cmp(&(x - b).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// Try to interpret a slice of consecutive lines as a table.
/// Returns Some(rows) if the lines look like a table, None otherwise.
fn try_as_table(lines: &[&TextLine]) -> Option<Vec<Vec<String>>> {
    if lines.len() < 2 {
        return None;
    }

    // Collect all word x-start positions
    let all_x: Vec<f64> = lines
        .iter()
        .flat_map(|l| l.words.iter().map(|w| w.x))
        .collect();

    let cols = cluster_columns(&all_x, 8.0);
    if cols.len() < 2 {
        return None;
    }

    // Count how many lines have words aligned to ≥2 distinct columns
    let aligned = lines
        .iter()
        .filter(|line| {
            let mut used_cols = std::collections::HashSet::new();
            for w in &line.words {
                used_cols.insert(nearest_col(w.x, &cols));
            }
            used_cols.len() >= 2
        })
        .count();

    // Require ≥ 2/3 of lines to be multi-column, and at least 2 such lines
    if aligned < 2 || aligned * 3 < lines.len() * 2 {
        return None;
    }

    // Build table rows: merge words that fall into the same cell
    let rows: Vec<Vec<String>> = lines
        .iter()
        .map(|line| {
            let mut cells: Vec<String> = vec![String::new(); cols.len()];
            for word in &line.words {
                let ci = nearest_col(word.x, &cols);
                if !cells[ci].is_empty() {
                    cells[ci].push(' ');
                }
                cells[ci].push_str(&word.text);
            }
            cells
        })
        .collect();

    Some(rows)
}

/// Check whether rectangles suggest a grid (table borders).
fn rects_suggest_table(rects: &[(f64, f64, f64, f64)]) -> bool {
    rects.len() >= 4
}

// ---------------------------------------------------------------------------
// Markdown rendering
// ---------------------------------------------------------------------------

fn render_table(writer: &mut dyn Write, rows: &[Vec<String>]) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return Ok(());
    }

    for (i, row) in rows.iter().enumerate() {
        let cells: Vec<String> = (0..col_count)
            .map(|ci| {
                row.get(ci)
                    .map(|s| s.replace('|', "\\|"))
                    .unwrap_or_default()
            })
            .collect();
        writeln!(writer, "| {} |", cells.join(" | "))?;

        // Insert separator after first row (header)
        if i == 0 {
            let sep: Vec<&str> = (0..col_count).map(|_| "---").collect();
            writeln!(writer, "| {} |", sep.join(" | "))?;
        }
    }
    writeln!(writer)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Main page content renderer
// ---------------------------------------------------------------------------

/// Compute the median y-gap between consecutive lines (typical line height).
fn typical_line_spacing(lines: &[TextLine]) -> f64 {
    if lines.len() < 2 {
        return 14.0;
    }
    let mut gaps: Vec<f64> = lines
        .windows(2)
        .map(|w| (w[0].y - w[1].y).abs())
        .filter(|&g| g > 1.0)
        .collect();
    if gaps.is_empty() {
        return 14.0;
    }
    gaps.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    gaps[gaps.len() / 2]
}

fn line_to_string(line: &TextLine) -> String {
    line.words
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_bullet_line(s: &str) -> bool {
    s.starts_with('•')
        || s.starts_with('●')
        || s.starts_with('○')
        || s.starts_with('–')
        || s.starts_with("- ")
        || s.starts_with("* ")
}

fn write_page_content(writer: &mut dyn Write, page: PageData) -> Result<()> {
    let has_table_rects = rects_suggest_table(&page.rects);
    let words = build_words(page.glyphs);
    let lines = build_lines(words);

    if lines.is_empty() {
        return Ok(());
    }

    let spacing = typical_line_spacing(&lines);
    // A gap larger than this threshold signals a paragraph break.
    // Use 1.4× median spacing; tighten to avoid joining across section breaks.
    let para_gap = spacing * 1.4;

    let mut i = 0;
    while i < lines.len() {
        // --- Table detection: try to grow a table region from i ---
        let mut table_end = i + 1;
        while table_end <= lines.len() {
            let slice: Vec<&TextLine> = lines[i..table_end].iter().collect();
            if try_as_table(&slice).is_none() && !(has_table_rects && table_end - i >= 2) {
                break;
            }
            table_end += 1;
        }
        table_end -= 1;

        if table_end > i + 1 {
            let slice: Vec<&TextLine> = lines[i..table_end].iter().collect();
            if let Some(rows) = try_as_table(&slice) {
                render_table(writer, &rows)?;
                i = table_end;
                continue;
            }
        }

        // --- Special single-line elements (bullets, numbered lists) ---
        let first_text = line_to_string(&lines[i]);
        let first_trimmed = first_text.trim();

        if is_bullet_line(first_trimmed) {
            let content = if first_trimmed.starts_with("- ") || first_trimmed.starts_with("* ") {
                first_trimmed[2..].trim()
            } else {
                first_trimmed[first_trimmed.chars().next().unwrap().len_utf8()..].trim()
            };
            writeln!(writer, "- {content}")?;
            i += 1;
            continue;
        }

        if let Some(content) = strip_numbered_prefix(first_trimmed) {
            writeln!(writer, "1. {content}")?;
            i += 1;
            continue;
        }

        // --- Paragraph grouping: accumulate lines until a break condition ---
        let mut para_lines: Vec<&TextLine> = vec![&lines[i]];
        let mut j = i + 1;

        while j < lines.len() {
            let y_gap = (lines[j - 1].y - lines[j].y).abs();

            // Large vertical gap → paragraph break
            if y_gap > para_gap {
                break;
            }

            let next_text = line_to_string(&lines[j]);
            let next_trimmed = next_text.trim();

            // Next line is a list item or starts a table → break
            if is_bullet_line(next_trimmed) || strip_numbered_prefix(next_trimmed).is_some() {
                break;
            }
            if j + 1 < lines.len() {
                let two: Vec<&TextLine> = lines[j..j + 2].iter().collect();
                if try_as_table(&two).is_some() {
                    break;
                }
            }

            para_lines.push(&lines[j]);
            j += 1;
        }

        write_paragraph(writer, &para_lines)?;
        i = j;
    }

    Ok(())
}

/// Join a group of consecutive lines into a single paragraph and write it.
fn write_paragraph(writer: &mut dyn Write, lines: &[&TextLine]) -> Result<()> {
    let mut para = String::new();

    for line in lines {
        let t = line_to_string(line);
        let t = t.trim();
        if t.is_empty() {
            continue;
        }

        // Handle hyphenated line breaks: "implemen-" + "tation" → "implementation"
        if para.ends_with('-') {
            let prev_alpha = para
                .chars()
                .rev()
                .nth(1)
                .map(|c| c.is_alphabetic())
                .unwrap_or(false);
            let next_lower = t.chars().next().map(|c| c.is_lowercase()).unwrap_or(false);
            if prev_alpha && next_lower {
                para.pop(); // remove hyphen
                para.push_str(t);
                continue;
            }
        }

        if !para.is_empty() {
            para.push(' ');
        }
        para.push_str(t);
    }

    let para = para.trim().to_string();
    if para.is_empty() {
        return Ok(());
    }

    // Single isolated line → check for heading
    if lines.len() == 1 && is_heading_candidate(&para) {
        writeln!(writer, "### {para}")?;
        writeln!(writer)?;
        return Ok(());
    }

    writeln!(writer, "{para}")?;
    writeln!(writer)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Text helpers (shared with structured text path)
// ---------------------------------------------------------------------------

fn is_heading_candidate(line: &str) -> bool {
    let len = line.len();
    if !(2..=80).contains(&len) {
        return false;
    }
    let last = line.chars().last().unwrap();
    if matches!(last, '.' | ',' | ';' | '!' | '?' | ')') {
        return false;
    }
    let first = line.chars().next().unwrap();
    if !first.is_uppercase() && !first.is_ascii_digit() {
        return false;
    }
    line.split_whitespace().count() <= 10
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
