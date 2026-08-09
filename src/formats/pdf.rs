use std::collections::{HashMap, HashSet};
use std::io::Write;

use pdf_extract::{
    ColorSpace, Document, MediaBox, Object, OutputDev, OutputError, Path, PathOp, Transform,
    output_doc,
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

        // Build words/lines for every page up front. This is needed so headings can be
        // detected from font size relative to the whole document, and so repeated
        // running headers/footers can be spotted across pages before rendering.
        let mut pages: Vec<PageLines> = collector
            .pages
            .into_iter()
            .map(|p| {
                let words = build_words(p.glyphs);
                let lines = build_lines(words);
                let had_content = !lines.is_empty();
                PageLines {
                    lines,
                    rects: p.rects,
                    media_box: p.media_box,
                    had_content,
                }
            })
            .collect();

        let body_size = estimate_body_font_size(&pages);
        let repeated = detect_repeated_header_footer(&pages);
        if !repeated.is_empty() {
            for page in &mut pages {
                strip_repeated_lines(&mut page.lines, page.media_box, &repeated);
            }
        }

        let total_pages = pages.len();
        for (i, page) in pages.into_iter().enumerate() {
            writeln!(writer, "## Page {}", i + 1)?;
            writeln!(writer)?;

            if !page.had_content {
                writeln!(writer, "*Empty page*")?;
            } else if !page.lines.is_empty() {
                write_page_content(writer, page.lines, &page.rects, body_size)?;
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
    font_size: f64,
    ch: String,
}

/// (llx, lly, urx, ury) page bounds, in the same coordinate space as glyph positions.
type MediaBoxDims = (f64, f64, f64, f64);

struct PageData {
    glyphs: Vec<Glyph>,
    rects: Vec<(f64, f64, f64, f64)>, // (x, y, width, height)
    media_box: MediaBoxDims,
}

struct PageLines {
    lines: Vec<TextLine>,
    rects: Vec<(f64, f64, f64, f64)>,
    media_box: MediaBoxDims,
    had_content: bool,
}

struct PageCollector {
    pages: Vec<PageData>,
    current_glyphs: Vec<Glyph>,
    current_rects: Vec<(f64, f64, f64, f64)>,
    current_media_box: MediaBoxDims,
}

impl PageCollector {
    fn new() -> Self {
        Self {
            pages: Vec::new(),
            current_glyphs: Vec::new(),
            current_rects: Vec::new(),
            current_media_box: (0.0, 0.0, 0.0, 0.0),
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
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> std::result::Result<(), OutputError> {
        self.current_glyphs.clear();
        self.current_rects.clear();
        self.current_media_box = (media_box.llx, media_box.lly, media_box.urx, media_box.ury);
        Ok(())
    }

    fn end_page(&mut self) -> std::result::Result<(), OutputError> {
        self.pages.push(PageData {
            glyphs: std::mem::take(&mut self.current_glyphs),
            rects: std::mem::take(&mut self.current_rects),
            media_box: self.current_media_box,
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
        // The effective rendered glyph height also scales with the text matrix,
        // so use `scale` (not the raw, possibly-normalized `font_size`) as the
        // heading/body-text size signal.
        let rendered_size = font_size.abs() * scale;
        self.current_glyphs.push(Glyph {
            x,
            y,
            advance,
            font_size: rendered_size,
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
    font_size: f64,
    text: String,
}

struct TextLine {
    y: f64,
    /// Representative font size for the line (max of its words' sizes), used for
    /// heading detection and to avoid merging differently-sized text into one paragraph.
    font_size: f64,
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
    let mut w_font = glyphs[0].font_size;
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
                font_size: w_font,
                text: buf.trim().to_string(),
            });
            buf.clear();
            wx = glyph.x;
            wy = glyph.y;
            w_font = glyph.font_size;
        } else if new_word {
            buf.clear();
            wx = glyph.x;
            wy = glyph.y;
            w_font = glyph.font_size;
        }

        if buf.is_empty() {
            wx = glyph.x;
            wy = glyph.y;
            w_font = glyph.font_size;
        }

        buf.push_str(&glyph.ch);
        prev_x_end = glyph.x + glyph.advance.max(1.0);
        prev_y = glyph.y;
    }

    if !buf.trim().is_empty() {
        words.push(Word {
            x: wx,
            y: wy,
            font_size: w_font,
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
            if word.font_size > last.font_size {
                last.font_size = word.font_size;
            }
            last.words.push(word);
            continue;
        }
        lines.push(TextLine {
            y: word.y,
            font_size: word.font_size,
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
// Body font size estimation & heading detection
// ---------------------------------------------------------------------------

/// Estimate the document's normal body-text font size as the most common
/// line font size (rounded to the nearest 0.5pt), across all pages.
fn estimate_body_font_size(pages: &[PageLines]) -> f64 {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for page in pages {
        for line in &page.lines {
            let bucket = (line.font_size * 2.0).round() as i64;
            if bucket <= 0 {
                continue;
            }
            *counts.entry(bucket).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .max_by_key(|&(_, c)| c)
        .map(|(b, _)| b as f64 / 2.0)
        .unwrap_or(10.0)
}

/// Map a line's font size (relative to the body text size) to a heading level.
fn heading_level_for_font(size: f64, body_size: f64) -> Option<u8> {
    if body_size <= 0.0 || size <= 0.0 {
        return None;
    }
    let ratio = size / body_size;
    if ratio >= 1.8 {
        Some(1)
    } else if ratio >= 1.4 {
        Some(2)
    } else if ratio >= 1.15 {
        Some(3)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Repeated header/footer detection
// ---------------------------------------------------------------------------

/// Normalize text for cross-page repetition comparison: lowercase, and collapse
/// any run of digits into a single `#` placeholder so page numbers like
/// "Page 3 of 42" and "Page 4 of 42" are recognized as the same running element.
fn normalize_repeat_text(s: &str) -> String {
    let mut out = String::new();
    let mut last_was_digit = false;
    for c in s.trim().chars() {
        if c.is_ascii_digit() {
            if !last_was_digit {
                out.push('#');
            }
            last_was_digit = true;
        } else {
            out.push(c.to_ascii_lowercase());
            last_was_digit = false;
        }
    }
    out
}

fn is_in_header_zone(y: f64, media_box: MediaBoxDims) -> bool {
    let (_, lly, _, ury) = media_box;
    let height = (ury - lly).max(1.0);
    y > ury - height * 0.08
}

fn is_in_footer_zone(y: f64, media_box: MediaBoxDims) -> bool {
    let (_, lly, _, ury) = media_box;
    let height = (ury - lly).max(1.0);
    y < lly + height * 0.08
}

/// Find lines (running headers/footers, page numbers) whose normalized text
/// repeats in the top/bottom margin across a majority of pages. Only meaningful
/// with at least 3 pages.
fn detect_repeated_header_footer(pages: &[PageLines]) -> HashSet<String> {
    if pages.len() < 3 {
        return HashSet::new();
    }

    let mut seen_on: HashMap<String, HashSet<usize>> = HashMap::new();
    for (page_idx, page) in pages.iter().enumerate() {
        for line in &page.lines {
            let text = line_to_string(line);
            let trimmed = text.trim();
            if trimmed.is_empty() || trimmed.chars().count() > 80 {
                continue;
            }
            if !is_in_header_zone(line.y, page.media_box)
                && !is_in_footer_zone(line.y, page.media_box)
            {
                continue;
            }
            let norm = normalize_repeat_text(trimmed);
            if norm.is_empty() {
                continue;
            }
            seen_on.entry(norm).or_default().insert(page_idx);
        }
    }

    let threshold = (pages.len() * 6 / 10).max(3);
    seen_on
        .into_iter()
        .filter(|(_, on_pages)| on_pages.len() >= threshold)
        .map(|(text, _)| text)
        .collect()
}

fn strip_repeated_lines(
    lines: &mut Vec<TextLine>,
    media_box: MediaBoxDims,
    repeated: &HashSet<String>,
) {
    if repeated.is_empty() {
        return;
    }
    lines.retain(|line| {
        let text = line_to_string(line);
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return true;
        }
        if !is_in_header_zone(line.y, media_box) && !is_in_footer_zone(line.y, media_box) {
            return true;
        }
        !repeated.contains(&normalize_repeat_text(trimmed))
    });
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
            let mut used_cols = HashSet::new();
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

fn write_page_content(
    writer: &mut dyn Write,
    lines: Vec<TextLine>,
    rects: &[(f64, f64, f64, f64)],
    body_size: f64,
) -> Result<()> {
    let has_table_rects = rects_suggest_table(rects);

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

            // A jump in font size (e.g. a heading immediately followed by body
            // text with little vertical gap) also signals a break.
            if (lines[j].font_size - para_lines[0].font_size).abs() > 0.5 {
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

        write_paragraph(writer, &para_lines, body_size)?;
        i = j;
    }

    Ok(())
}

/// Join a group of consecutive lines into a single paragraph and write it.
fn write_paragraph(writer: &mut dyn Write, lines: &[&TextLine], body_size: f64) -> Result<()> {
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

    // Single isolated line → check for heading, preferring the font-size signal
    // (relative to body text) over the punctuation/capitalization heuristic.
    if lines.len() == 1 {
        if let Some(level) = heading_level_for_font(lines[0].font_size, body_size)
            && para.chars().count() <= 200
        {
            let hashes = "#".repeat(level as usize);
            writeln!(writer, "{hashes} {para}")?;
            writeln!(writer)?;
            return Ok(());
        }

        if is_heading_candidate(&para) {
            writeln!(writer, "### {para}")?;
            writeln!(writer)?;
            return Ok(());
        }
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

fn pdf_object_to_string(obj: &Object) -> String {
    match obj {
        Object::String(bytes, _) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal, valid single-content-stream-per-page PDF using only
    /// the built-in Helvetica font, for exercising the layout-based parser.
    fn make_pdf(pages: &[&str], media_box: &str) -> Vec<u8> {
        let n_pages = pages.len();
        let font_obj_num = 3 + n_pages * 2;
        let kids: Vec<String> = (0..n_pages).map(|i| format!("{} 0 R", 3 + i * 2)).collect();

        let mut objs: Vec<(usize, String)> = Vec::new();
        objs.push((1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()));
        objs.push((
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} >>",
                kids.join(" "),
                n_pages
            ),
        ));
        for (i, content) in pages.iter().enumerate() {
            let page_num = 3 + i * 2;
            let content_num = page_num + 1;
            objs.push((
                page_num,
                format!(
                    "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 {font_obj_num} 0 R >> >> /MediaBox {media_box} /Contents {content_num} 0 R >>"
                ),
            ));
            objs.push((
                content_num,
                format!(
                    "<< /Length {} >>\nstream\n{}\nendstream",
                    content.as_bytes().len(),
                    content
                ),
            ));
        }
        objs.push((
            font_obj_num,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ));

        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = vec![0usize; objs.len() + 2];
        for (num, body) in &objs {
            offsets[*num] = out.as_bytes().len();
            out.push_str(&format!("{num} 0 obj\n{body}\nendobj\n"));
        }
        let xref_offset = out.as_bytes().len();
        let max_num = objs.iter().map(|(n, _)| *n).max().unwrap();
        out.push_str(&format!("xref\n0 {}\n", max_num + 1));
        out.push_str("0000000000 65535 f \n");
        for i in 1..=max_num {
            out.push_str(&format!(
                "{:010} 00000 n \n",
                offsets.get(i).copied().unwrap_or(0)
            ));
        }
        out.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF",
            max_num + 1,
            xref_offset
        ));
        out.into_bytes()
    }

    fn convert(pages: &[&str], media_box: &str) -> String {
        let pdf = make_pdf(pages, media_box);
        let mut out = Vec::new();
        PdfConverter.convert(&pdf, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_large_font_line_becomes_h1() {
        let page = "BT /F1 24 Tf 20 260 Td (Big Heading) Tj ET\n\
             BT /F1 10 Tf 20 220 Td (This is normal body text on the page.) Tj ET";
        let out = convert(&[page], "[0 0 300 300]");
        assert!(out.contains("# Big Heading"), "missing h1 in:\n{out}");
        assert!(
            out.contains("This is normal body text on the page."),
            "missing body text in:\n{out}"
        );
    }

    #[test]
    fn test_medium_font_line_becomes_h3() {
        let page = "BT /F1 10 Tf 20 260 Td (This is normal body text setting the baseline.) Tj ET\n\
             BT /F1 13 Tf 20 220 Td (Subheading) Tj ET\n\
             BT /F1 10 Tf 20 200 Td (More normal body text here to confirm.) Tj ET";
        let out = convert(&[page], "[0 0 300 300]");
        assert!(out.contains("### Subheading"), "missing h3 in:\n{out}");
    }

    #[test]
    fn test_repeated_footer_stripped_across_pages() {
        let mk = |body: &str, n: u32| {
            format!(
                "BT /F1 10 Tf 20 220 Td ({body}) Tj ET\nBT /F1 8 Tf 20 10 Td (Confidential - Page {n}) Tj ET"
            )
        };
        let p1 = mk("Body text on page one.", 1);
        let p2 = mk("Body text on page two.", 2);
        let p3 = mk("Body text on page three.", 3);
        let out = convert(&[p1.as_str(), p2.as_str(), p3.as_str()], "[0 0 300 300]");
        assert!(
            !out.to_lowercase().contains("confidential"),
            "repeated footer should have been stripped:\n{out}"
        );
        assert!(out.contains("Body text on page one."));
        assert!(out.contains("Body text on page two."));
        assert!(out.contains("Body text on page three."));
    }

    #[test]
    fn test_footer_not_stripped_below_page_threshold() {
        // Only 2 pages: repetition detection requires >= 3 pages, so a
        // shared footer line should be left intact rather than guessed away.
        let mk = |body: &str| {
            format!(
                "BT /F1 10 Tf 20 220 Td ({body}) Tj ET\nBT /F1 8 Tf 20 10 Td (Confidential) Tj ET"
            )
        };
        let p1 = mk("Body text on page one.");
        let p2 = mk("Body text on page two.");
        let out = convert(&[p1.as_str(), p2.as_str()], "[0 0 300 300]");
        assert!(
            out.to_lowercase().contains("confidential"),
            "footer should be kept with fewer than 3 pages:\n{out}"
        );
    }
}
