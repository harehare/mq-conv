use std::collections::HashMap;
use std::io::Write;

use unpdf::render::{CleanupPreset, PageMarkerStyle, RenderOptions, to_markdown};
use unpdf::{Document, PdfParser};

use crate::converter::Converter;
use crate::error::{Error, Result};

const PAGE_MARKER_PREFIX: &str = "<!-- page ";
const PAGE_MARKER_SUFFIX: &str = " -->";

pub struct PdfConverter;

impl Converter for PdfConverter {
    fn format_name(&self) -> &'static str {
        "pdf"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let doc = PdfParser::from_bytes(input)
            .and_then(|parser| parser.parse())
            .map_err(|e| Error::Conversion {
                format: "pdf",
                message: e.to_string(),
            })?;

        if doc.is_empty() || doc.plain_text().trim().is_empty() {
            writeln!(
                writer,
                "*PDF contains no extractable text (may be scanned/image-based)*"
            )?;
            return Ok(());
        }

        let options = RenderOptions::new()
            .with_cleanup_preset(CleanupPreset::Standard)
            .with_page_markers(PageMarkerStyle::Comment);

        let markdown = to_markdown(&doc, &options).map_err(|e| Error::Conversion {
            format: "pdf",
            message: e.to_string(),
        })?;

        let markdown = fix_table_separators(&markdown);
        let markdown = strip_repeated_page_lines(&markdown, doc.page_count() as usize);

        let body_title = body_leading_heading(&markdown);
        writeln!(writer, "{}", write_metadata(&doc, body_title.as_deref()))?;
        write!(writer, "{markdown}")?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Markdown post-processing: fixes for known gaps in unpdf's renderer.
// ---------------------------------------------------------------------------

/// Split a `| a | b |` table row into trimmed cells, or `None` if not one.
fn table_cells(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    if t.len() < 2 || !t.starts_with('|') || !t.ends_with('|') {
        return None;
    }
    let inner = &t[1..t.len() - 1];
    Some(inner.split('|').map(|c| c.trim().to_string()).collect())
}

fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

fn render_row(cells: &[String]) -> String {
    format!("| {} |", cells.join(" | "))
}

fn render_separator(col_count: usize) -> String {
    render_row(&vec!["---".to_string(); col_count])
}

/// unpdf sometimes omits the `| --- | --- |` separator (invalid CommonMark)
/// and folds a preceding heading into the table's first row (one non-empty
/// cell). This inserts the separator and lifts a misplaced title back out.
fn fix_table_separators(markdown: &str) -> String {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;

    while i < lines.len() {
        let Some(first_cells) = table_cells(lines[i]) else {
            out.push(lines[i].to_string());
            i += 1;
            continue;
        };

        let mut block: Vec<Vec<String>> = vec![first_cells];
        let mut j = i + 1;
        loop {
            if let Some(cells) = lines.get(j).and_then(|l| table_cells(l)) {
                block.push(cells);
                j += 1;
                continue;
            }
            // A blank line can separate a spurious title row from the real table.
            if lines.get(j).is_some_and(|l| l.trim().is_empty())
                && let Some(cells) = lines.get(j + 1).and_then(|l| table_cells(l))
            {
                block.push(cells);
                j += 2;
                continue;
            }
            break;
        }

        if block.len() >= 2 && !is_separator_row(&block[1]) {
            let first_non_empty = block[0].iter().filter(|c| !c.is_empty()).count();
            let header_non_empty = block[1].iter().filter(|c| !c.is_empty()).count();
            if first_non_empty == 1 && block[0].len() > 1 && header_non_empty > 1 {
                let title = block[0]
                    .iter()
                    .find(|c| !c.is_empty())
                    .cloned()
                    .unwrap_or_default();
                out.push(format!("## {title}"));
                out.push(String::new());
                block.remove(0);
            }
        }

        if !block.is_empty() {
            let col_count = block[0].len();
            let has_separator = block.get(1).is_some_and(|r| is_separator_row(r));
            out.push(render_row(&block[0]));
            if !has_separator {
                out.push(render_separator(col_count));
            }
            for row in &block[1..] {
                out.push(render_row(row));
            }
        }

        i = j;
    }

    out.join("\n")
}

/// Lowercase and collapse digit runs to `#` so "Page 3 of 42" and "Page 4 of
/// 42" compare equal across pages.
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

fn is_page_marker(line: &str) -> bool {
    line.starts_with(PAGE_MARKER_PREFIX) && line.ends_with(PAGE_MARKER_SUFFIX)
}

/// Split rendered Markdown into a preamble plus one `(marker, lines)` chunk
/// per page, using the `<!-- page N -->` markers.
fn split_into_pages(lines: &[&str]) -> (Vec<String>, Vec<(String, Vec<String>)>) {
    let mut preamble = Vec::new();
    let mut pages: Vec<(String, Vec<String>)> = Vec::new();

    for &line in lines {
        if is_page_marker(line) {
            pages.push((line.to_string(), Vec::new()));
            continue;
        }
        match pages.last_mut() {
            Some((_, page)) => page.push(line.to_string()),
            None => preamble.push(line.to_string()),
        }
    }

    (preamble, pages)
}

/// unpdf has no equivalent of running-header/footer stripping. Without glyph
/// positions this approximates it: a short line repeating (ignoring digits)
/// as the first/last line of most pages is dropped.
fn strip_repeated_page_lines(markdown: &str, page_count: usize) -> String {
    if page_count < 3 {
        return markdown.to_string();
    }

    let lines: Vec<&str> = markdown.lines().collect();
    let (preamble, mut pages) = split_into_pages(&lines);
    if pages.len() < 3 {
        return markdown.to_string();
    }

    // A long line is body text, not a running header/footer, even at page edges.
    const MAX_CANDIDATE_LEN: usize = 80;

    let mut counts: HashMap<String, usize> = HashMap::new();
    for (_, page) in &pages {
        let mut candidates: Vec<String> = Vec::new();
        for l in [
            page.iter().find(|l| !l.trim().is_empty()),
            page.iter().rev().find(|l| !l.trim().is_empty()),
        ]
        .into_iter()
        .flatten()
        {
            if l.trim().chars().count() > MAX_CANDIDATE_LEN {
                continue;
            }
            let norm = normalize_repeat_text(l);
            if !norm.is_empty() && !candidates.contains(&norm) {
                candidates.push(norm);
            }
        }
        for c in candidates {
            *counts.entry(c).or_insert(0) += 1;
        }
    }

    let threshold = (pages.len() * 6 / 10).max(3);
    let repeated: std::collections::HashSet<String> = counts
        .into_iter()
        .filter(|&(_, n)| n >= threshold)
        .map(|(text, _)| text)
        .collect();
    if repeated.is_empty() {
        return markdown.to_string();
    }

    for (_, page) in &mut pages {
        if let Some(idx) = page.iter().position(|l| !l.trim().is_empty())
            && repeated.contains(&normalize_repeat_text(&page[idx]))
        {
            page[idx].clear();
        }
        if let Some(idx) = page.iter().rposition(|l| !l.trim().is_empty())
            && repeated.contains(&normalize_repeat_text(&page[idx]))
        {
            page[idx].clear();
        }
    }

    let mut out = preamble;
    for (marker, page) in pages {
        out.push(marker);
        out.extend(page);
    }

    // Collapse blank runs left behind by cleared lines.
    let mut collapsed: Vec<String> = Vec::with_capacity(out.len());
    for line in out {
        if line.trim().is_empty() && collapsed.last().is_some_and(|l: &String| l.trim().is_empty())
        {
            continue;
        }
        collapsed.push(line);
    }
    collapsed.join("\n")
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

fn titles_match(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn write_metadata(doc: &Document, body_title: Option<&str>) -> String {
    let meta = &doc.metadata;
    let mut out = String::new();

    let meta_title = meta
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty());

    match (meta_title, body_title) {
        (_, Some(_)) => {}
        (Some(title), None) => out.push_str(&format!("# {title}\n\n")),
        (None, None) => out.push_str("# PDF Document\n\n"),
    }

    let mut fields: Vec<(&str, String)> = Vec::new();
    if let Some(title) = meta_title
        && body_title.is_some_and(|b| !titles_match(title, b))
    {
        fields.push(("Title", title.to_string()));
    }
    if let Some(author) = &meta.author
        && !author.trim().is_empty()
    {
        fields.push(("Author", author.clone()));
    }
    if let Some(subject) = &meta.subject
        && !subject.trim().is_empty()
    {
        fields.push(("Subject", subject.clone()));
    }
    if let Some(creator) = &meta.creator
        && !creator.trim().is_empty()
    {
        fields.push(("Creator", creator.clone()));
    }
    if let Some(producer) = &meta.producer
        && !producer.trim().is_empty()
    {
        fields.push(("Producer", producer.clone()));
    }
    if let Some(created) = &meta.created {
        fields.push(("Created", created.to_rfc3339()));
    }
    if let Some(modified) = &meta.modified {
        fields.push(("Modified", modified.to_rfc3339()));
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
mod tests {
    use super::*;

    #[test]
    fn test_fix_table_separators_inserts_missing_separator() {
        let md = "| Name | Age |\n| Alice | 30 |\n| Bob | 40 |";
        let out = fix_table_separators(md);
        assert_eq!(
            out,
            "| Name | Age |\n| --- | --- |\n| Alice | 30 |\n| Bob | 40 |"
        );
    }

    #[test]
    fn test_fix_table_separators_leaves_existing_separator_alone() {
        let md = "| Name | Age |\n| --- | --- |\n| Alice | 30 |";
        assert_eq!(fix_table_separators(md), md);
    }

    #[test]
    fn test_fix_table_separators_lifts_title_row_out_of_table() {
        let md = "| Sales Report |  |  |  |\n\n| Name | Region | Q1 | Q2 |\n| Alice | East | 100 | 120 |";
        let out = fix_table_separators(md);
        assert_eq!(
            out,
            "## Sales Report\n\n| Name | Region | Q1 | Q2 |\n| --- | --- | --- | --- |\n| Alice | East | 100 | 120 |"
        );
    }

    #[test]
    fn test_fix_table_separators_does_not_lift_multi_cell_first_row() {
        // First row has more than one non-empty cell, so it's a real header,
        // not a misplaced title — must not be pulled out of the table.
        let md = "| Region | Total |\n| East | 120 |\n| West | 95 |";
        let out = fix_table_separators(md);
        assert_eq!(
            out,
            "| Region | Total |\n| --- | --- |\n| East | 120 |\n| West | 95 |"
        );
    }

    #[test]
    fn test_fix_table_separators_single_row_gets_separator() {
        let md = "| Name | Age |";
        assert_eq!(fix_table_separators(md), "| Name | Age |\n| --- | --- |");
    }

    #[test]
    fn test_fix_table_separators_two_independent_tables() {
        let md = "| A | B |\n| 1 | 2 |\n\nSome paragraph in between.\n\n| C | D |\n| 3 | 4 |";
        let out = fix_table_separators(md);
        assert_eq!(
            out,
            "| A | B |\n| --- | --- |\n| 1 | 2 |\n\nSome paragraph in between.\n\n| C | D |\n| --- | --- |\n| 3 | 4 |"
        );
    }

    #[test]
    fn test_table_cells_none_for_plain_text() {
        assert_eq!(table_cells("It costs $5 or $10 depending on size."), None);
        assert_eq!(table_cells("no leading pipe |"), None);
        assert_eq!(table_cells("| no trailing pipe"), None);
    }

    #[test]
    fn test_table_cells_parses_row() {
        assert_eq!(
            table_cells("| Name | Age |"),
            Some(vec!["Name".to_string(), "Age".to_string()])
        );
    }

    #[test]
    fn test_is_separator_row() {
        assert!(is_separator_row(&["---".to_string(), ":--:".to_string()]));
        assert!(!is_separator_row(&["Name".to_string(), "---".to_string()]));
        assert!(!is_separator_row(&[]));
    }

    #[test]
    fn test_normalize_repeat_text() {
        assert_eq!(normalize_repeat_text("Page 3 of 42"), "page # of #");
        assert_eq!(normalize_repeat_text("  Confidential  "), "confidential");
    }

    #[test]
    fn test_strip_repeated_page_lines_removes_running_header() {
        let md = "<!-- page 1 -->\n\nConfidential\nContent unique to the first page here.\n\
                  <!-- page 2 -->\n\nConfidential\nContent unique to the second page here.\n\
                  <!-- page 3 -->\n\nConfidential\nContent unique to the third page here.";
        let out = strip_repeated_page_lines(md, 3);
        assert!(!out.to_lowercase().contains("confidential"));
        assert!(out.contains("first page"));
        assert!(out.contains("second page"));
        assert!(out.contains("third page"));
    }

    #[test]
    fn test_strip_repeated_page_lines_ignores_middle_line_repetition() {
        // "Note" repeats on every page but sits in the middle, not at a page
        // edge, so it's not a header/footer candidate and must survive.
        let md = "<!-- page 1 -->\n\nFirst line one.\nNote\nLast line one.\n\
                  <!-- page 2 -->\n\nFirst line two.\nNote\nLast line two.\n\
                  <!-- page 3 -->\n\nFirst line three.\nNote\nLast line three.";
        let out = strip_repeated_page_lines(md, 3);
        assert_eq!(out.matches("Note").count(), 3);
    }

    #[test]
    fn test_strip_repeated_page_lines_requires_majority() {
        // Repeats on only 2 of 5 pages; threshold is (5*6/10).max(3) == 3, so
        // it must be left alone.
        let md = "<!-- page 1 -->\n\nBody A.\nStamp\n\
                  <!-- page 2 -->\n\nBody B.\nStamp\n\
                  <!-- page 3 -->\n\nBody C.\nZeta\n\
                  <!-- page 4 -->\n\nBody D.\nTheta\n\
                  <!-- page 5 -->\n\nBody E.\nOmega";
        let out = strip_repeated_page_lines(md, 5);
        assert_eq!(out.matches("Stamp").count(), 2);
    }

    #[test]
    fn test_strip_repeated_page_lines_removes_running_footer() {
        let md = "<!-- page 1 -->\n\nA paragraph describing findings specific to the first page of the report.\nConfidential - Page 1\n\
                  <!-- page 2 -->\n\nA different paragraph covering results found only on the second page.\nConfidential - Page 2\n\
                  <!-- page 3 -->\n\nYet another paragraph with content that only appears on the third page.\nConfidential - Page 3";
        let out = strip_repeated_page_lines(md, 3);
        assert!(out.contains("first page of the report"));
        assert!(out.contains("second page"));
        assert!(out.contains("third page"));
        assert!(
            !out.to_lowercase().contains("confidential"),
            "repeated footer should have been stripped:\n{out}"
        );
    }

    #[test]
    fn test_strip_repeated_page_lines_keeps_long_page_unique_first_line() {
        let md = "<!-- page 1 -->\n\nBody text unique to page 1 describing something important. A second line of body text on page 1.\n\
                  <!-- page 2 -->\n\nBody text unique to page 2 describing something important. A second line of body text on page 2.\n\
                  <!-- page 3 -->\n\nBody text unique to page 3 describing something important. A second line of body text on page 3.";
        let out = strip_repeated_page_lines(md, 3);
        assert!(out.contains("Body text unique to page 1 describing something important."));
        assert!(out.contains("Body text unique to page 2 describing something important."));
        assert!(out.contains("Body text unique to page 3 describing something important."));
    }

    #[test]
    fn test_strip_repeated_page_lines_below_threshold_pages_untouched() {
        let md = "<!-- page 1 -->\n\nBody one.\nConfidential\n\
                  <!-- page 2 -->\n\nBody two.\nConfidential";
        let out = strip_repeated_page_lines(md, 2);
        assert_eq!(out, md, "fewer than 3 pages should be left untouched");
    }

    /// Build a minimal, valid single-content-stream-per-page PDF using only
    /// the built-in Helvetica font.
    fn make_pdf(pages: &[&str], media_box: &str, info: Option<&str>) -> Vec<u8> {
        let n_pages = pages.len();
        let font_obj_num = 3 + n_pages * 2;
        let info_obj_num = font_obj_num + 1;
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
                    content.len(),
                    content
                ),
            ));
        }
        objs.push((
            font_obj_num,
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ));
        if let Some(info) = info {
            objs.push((info_obj_num, info.to_string()));
        }

        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = vec![0usize; objs.len() + 2];
        for (num, body) in &objs {
            offsets[*num] = out.len();
            out.push_str(&format!("{num} 0 obj\n{body}\nendobj\n"));
        }
        let xref_offset = out.len();
        let max_num = objs.iter().map(|(n, _)| *n).max().unwrap();
        out.push_str(&format!("xref\n0 {}\n", max_num + 1));
        out.push_str("0000000000 65535 f \n");
        for i in 1..=max_num {
            out.push_str(&format!(
                "{:010} 00000 n \n",
                offsets.get(i).copied().unwrap_or(0)
            ));
        }
        let info_entry = if info.is_some() {
            format!(" /Info {info_obj_num} 0 R")
        } else {
            String::new()
        };
        out.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R{info_entry} >>\nstartxref\n{}\n%%EOF",
            max_num + 1,
            xref_offset
        ));
        out.into_bytes()
    }

    fn convert(input: &[u8]) -> String {
        let mut out = Vec::new();
        PdfConverter.convert(input, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    /// Japanese PDF (Identity-H CIDFontType0, no `/ToUnicode`) — the shape that
    /// silently drops all text without a CIDSystemInfo/embedded-cmap fallback.
    const JA_SAMPLE: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pdf/sample_ja.pdf"));

    #[test]
    fn test_cjk_text_without_to_unicode_is_preserved() {
        let out = convert(JA_SAMPLE);
        assert!(
            out.contains("日本語"),
            "Japanese text missing from output:\n{out}"
        );
        assert!(
            out.contains("マルチバイト"),
            "multibyte text missing from output:\n{out}"
        );
    }

    #[test]
    fn test_basic_text_extraction() {
        let page = "BT /F1 10 Tf 20 220 Td (Hello world.) Tj ET";
        let out = convert(&make_pdf(&[page], "[0 0 300 300]", None));
        assert!(out.contains("Hello world."), "missing body text in:\n{out}");
    }

    #[test]
    fn test_metadata_is_rendered() {
        let page = "BT /F1 10 Tf 20 220 Td (Body.) Tj ET";
        let info = "<< /Title (My Title) /Author (Jane Doe) >>";
        let out = convert(&make_pdf(&[page], "[0 0 300 300]", Some(info)));
        assert!(out.contains("# My Title"), "missing title in:\n{out}");
        assert!(
            out.contains("**Author**: Jane Doe"),
            "missing author in:\n{out}"
        );
    }

    #[test]
    fn test_no_extractable_text_message() {
        let out = convert(&make_pdf(&[""], "[0 0 300 300]", None));
        assert!(
            out.contains("no extractable text"),
            "missing fallback message in:\n{out}"
        );
    }

    #[test]
    fn test_multi_page_pdf_has_page_markers_and_content() {
        let p1 = "BT /F1 10 Tf 20 220 Td (Page one content.) Tj ET";
        let p2 = "BT /F1 10 Tf 20 220 Td (Page two content.) Tj ET";
        let out = convert(&make_pdf(&[p1, p2], "[0 0 300 300]", None));
        assert!(out.contains("<!-- page 1 -->"));
        assert!(out.contains("<!-- page 2 -->"));
        assert!(out.contains("Page one content."));
        assert!(out.contains("Page two content."));
    }

    #[test]
    fn test_invalid_pdf_returns_error() {
        let mut out = Vec::new();
        assert!(PdfConverter.convert(b"not a pdf file", &mut out).is_err());
    }

    #[test]
    fn test_write_metadata_no_info_falls_back() {
        let doc = unpdf::Document::new();
        let out = write_metadata(&doc, None);
        assert!(out.starts_with("# PDF Document\n\n"));
        assert!(out.trim_end().ends_with("---"));
    }

    #[test]
    fn test_write_metadata_whitespace_title_falls_back() {
        let mut doc = unpdf::Document::new();
        doc.metadata.title = Some("   ".to_string());
        let out = write_metadata(&doc, None);
        assert!(out.starts_with("# PDF Document\n\n"));
    }

    #[test]
    fn test_write_metadata_only_present_fields_are_listed() {
        let mut doc = unpdf::Document::new();
        doc.metadata.title = Some("Report".to_string());
        doc.metadata.subject = Some("Q1 results".to_string());
        let out = write_metadata(&doc, None);
        assert!(out.contains("# Report"));
        assert!(out.contains("**Subject**: Q1 results"));
        assert!(!out.contains("Author"));
    }

    const TITLED_PAGE: &str = "BT /F1 24 Tf 20 250 Td (Quarterly Report) Tj ET \
         BT /F1 10 Tf 20 220 Td (This is the first paragraph of body text.) Tj ET \
         BT /F1 10 Tf 20 200 Td (This is the second paragraph of body text.) Tj ET \
         BT /F1 10 Tf 20 180 Td (This is the third paragraph of body text.) Tj ET \
         BT /F1 16 Tf 20 150 Td (A Subheading) Tj ET \
         BT /F1 10 Tf 20 130 Td (More body text under the subheading.) Tj ET";

    #[test]
    fn test_no_duplicate_title_when_info_title_absent() {
        let out = convert(&make_pdf(&[TITLED_PAGE], "[0 0 300 300]", None));
        assert_eq!(
            out.matches("# Quarterly Report").count(),
            1,
            "title should appear once, from the body heading, not also as a synthetic preamble:\n{out}"
        );
        assert!(!out.contains("PDF Document"), "no title fallback should leak through:\n{out}");
    }

    #[test]
    fn test_no_duplicate_title_when_info_title_matches_body() {
        let info = "<< /Title (Quarterly Report) >>";
        let out = convert(&make_pdf(&[TITLED_PAGE], "[0 0 300 300]", Some(info)));
        assert_eq!(
            out.matches("# Quarterly Report").count(),
            1,
            "matching Info-dict title and body heading should collapse into one heading:\n{out}"
        );
    }

    #[test]
    fn test_differing_info_title_kept_as_field() {
        let info = "<< /Title (Q3 FY24 Report - Draft) >>";
        let out = convert(&make_pdf(&[TITLED_PAGE], "[0 0 300 300]", Some(info)));
        assert!(
            out.contains("# Quarterly Report"),
            "body heading should still lead the document:\n{out}"
        );
        assert!(
            out.contains("**Title**: Q3 FY24 Report - Draft"),
            "differing Info-dict title should survive as a metadata field:\n{out}"
        );
    }
}
