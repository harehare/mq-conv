use super::*;

/// Build a minimal, valid single-content-stream-per-page PDF using only the
/// built-in Helvetica font (no /Widths: exercises the metric fallback).
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
                "<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 {font_obj_num} 0 R /F2 {} 0 R >> >> /MediaBox {media_box} /Contents {content_num} 0 R >>",
                font_obj_num + 2
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
    objs.push((
        font_obj_num + 2,
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_string(),
    ));
    if let Some(info) = info {
        objs.push((info_obj_num, info.to_string()));
    }

    let mut out = String::from("%PDF-1.4\n");
    let max_num = objs.iter().map(|(n, _)| *n).max().unwrap();
    let mut offsets = vec![0usize; max_num + 1];
    for (num, body) in &objs {
        offsets[*num] = out.len();
        out.push_str(&format!("{num} 0 obj\n{body}\nendobj\n"));
    }
    let xref_offset = out.len();
    out.push_str(&format!("xref\n0 {}\n", max_num + 1));
    out.push_str("0000000000 65535 f \n");
    for off in offsets.iter().skip(1) {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    let info_entry = if info.is_some() {
        format!(" /Info {info_obj_num} 0 R")
    } else {
        String::new()
    };
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R{info_entry} >>\nstartxref\n{xref_offset}\n%%EOF",
        max_num + 1,
    ));
    out.into_bytes()
}

fn convert(input: &[u8]) -> String {
    let mut out = Vec::new();
    PdfConverter.convert(input, &mut out).unwrap();
    String::from_utf8(out).unwrap()
}

/// `BT /F1 <size> Tf <x> <y> Td (<text>) Tj ET`
fn text(font: &str, size: u32, x: u32, y: u32, s: &str) -> String {
    format!("BT /{font} {size} Tf {x} {y} Td ({s}) Tj ET ")
}

/// Japanese PDF (Identity-H CIDFontType0, no `/ToUnicode`) — the shape that
/// silently drops all text without a CIDSystemInfo-based fallback.
const JA_SAMPLE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/pdf/sample_ja.pdf"
));

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
    let page = text("F1", 10, 20, 220, "Hello world.");
    let out = convert(&make_pdf(&[&page], "[0 0 300 300]", None));
    assert!(out.contains("Hello world."), "missing body text in:\n{out}");
}

#[test]
fn test_metadata_is_rendered() {
    let page = text("F1", 10, 20, 220, "Body.");
    let info = "<< /Title (My Title) /Author (Jane Doe) /CreationDate (D:20240102030405Z) >>";
    let out = convert(&make_pdf(&[&page], "[0 0 300 300]", Some(info)));
    assert!(out.contains("# My Title"), "missing title in:\n{out}");
    assert!(
        out.contains("**Author**: Jane Doe"),
        "missing author in:\n{out}"
    );
    assert!(
        out.contains("**Created**: 2024-01-02T03:04:05Z"),
        "missing date in:\n{out}"
    );
}

#[test]
fn test_pdf_date_keeps_the_timezone_offset() {
    let cases = [
        ("D:20240102120000Z", "2024-01-02T12:00:00Z"),
        ("D:20240102120000+05'00'", "2024-01-02T12:00:00+05:00"),
        ("D:20240102120000-08'30'", "2024-01-02T12:00:00-08:30"),
        ("D:20240102120000+09'", "2024-01-02T12:00:00+09:00"),
        ("D:20240102120000+0530", "2024-01-02T12:00:00+05:30"),
        ("D:20240102120000", "2024-01-02T12:00:00"),
        ("D:2024", "2024-01-01T00:00:00"),
    ];
    for (input, expected) in cases {
        assert_eq!(format_pdf_date(input), expected, "{input}");
    }
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
    let p1 = text("F1", 10, 20, 220, "Page one content.");
    let p2 = text("F1", 10, 20, 220, "Page two content.");
    let out = convert(&make_pdf(&[&p1, &p2], "[0 0 300 300]", None));
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

fn titled_page() -> String {
    text("F1", 24, 20, 250, "Quarterly Report")
        + &text(
            "F1",
            10,
            20,
            220,
            "This is the first paragraph of body text.",
        )
        + &text(
            "F1",
            10,
            20,
            200,
            "This is the second paragraph of body text.",
        )
        + &text(
            "F1",
            10,
            20,
            180,
            "This is the third paragraph of body text.",
        )
        + &text("F1", 16, 20, 150, "A Subheading")
        + &text("F1", 10, 20, 130, "More body text under the subheading.")
}

#[test]
fn test_headings_from_font_size() {
    let out = convert(&make_pdf(&[&titled_page()], "[0 0 300 300]", None));
    assert!(out.contains("# Quarterly Report"), "missing H1 in:\n{out}");
    assert!(out.contains("## A Subheading"), "missing H2 in:\n{out}");
}

#[test]
fn test_no_duplicate_title_when_info_title_absent() {
    let out = convert(&make_pdf(&[&titled_page()], "[0 0 300 300]", None));
    assert_eq!(out.matches("# Quarterly Report").count(), 1, "{out}");
    assert!(!out.contains("PDF Document"), "{out}");
}

#[test]
fn test_no_duplicate_title_when_info_title_matches_body() {
    let info = "<< /Title (Quarterly Report) >>";
    let out = convert(&make_pdf(&[&titled_page()], "[0 0 300 300]", Some(info)));
    assert_eq!(out.matches("# Quarterly Report").count(), 1, "{out}");
}

#[test]
fn test_differing_info_title_kept_as_field() {
    let info = "<< /Title (Q3 FY24 Report - Draft) >>";
    let out = convert(&make_pdf(&[&titled_page()], "[0 0 300 300]", Some(info)));
    assert!(out.contains("# Quarterly Report"), "{out}");
    assert!(out.contains("**Title**: Q3 FY24 Report - Draft"), "{out}");
}

#[test]
fn test_running_header_and_footer_are_removed() {
    let page = |n: u32, body: &str| {
        text("F1", 9, 20, 285, "Confidential")
            + &text("F1", 10, 20, 150, body)
            + &text("F1", 9, 20, 10, &format!("Page {n} of 3"))
    };
    let (a, b, c) = (
        page(1, "Alpha findings for the first section."),
        page(2, "Bravo findings for the second section."),
        page(3, "Charlie findings for the third section."),
    );
    let out = convert(&make_pdf(&[&a, &b, &c], "[0 0 300 300]", None));
    assert!(out.contains("Alpha findings"), "{out}");
    assert!(out.contains("Charlie findings"), "{out}");
    assert!(
        !out.to_lowercase().contains("confidential"),
        "header kept:\n{out}"
    );
    assert!(!out.contains("of 3"), "footer kept:\n{out}");
}

#[test]
fn test_two_column_reading_order() {
    let mut page = String::new();
    for (i, y) in [250u32, 238, 226, 214].iter().enumerate() {
        page += &text(
            "F1",
            10,
            20,
            *y,
            &format!("Left column line {} has some ordinary prose", i + 1),
        );
        page += &text(
            "F1",
            10,
            260,
            *y,
            &format!("Right column line {} has some ordinary prose", i + 1),
        );
    }
    let out = convert(&make_pdf(&[&page], "[0 0 480 300]", None));
    let left_last = out.find("Left column line 4").expect(&out);
    let right_first = out.find("Right column line 1").expect(&out);
    assert!(left_last < right_first, "columns interleaved:\n{out}");
}

#[test]
fn test_ruled_table() {
    // 2 columns x 3 rows grid with drawn lines.
    let mut page = String::new();
    for y in [260, 240, 220, 200] {
        page += &format!("20 {y} m 220 {y} l S ");
    }
    for x in [20, 120, 220] {
        page += &format!("{x} 200 m {x} 260 l S ");
    }
    for (row, (a, b)) in [("Name", "Age"), ("Alice", "30"), ("Bob", "41")]
        .iter()
        .enumerate()
    {
        let y = 245 - row as u32 * 20;
        page += &text("F1", 10, 25, y, a);
        page += &text("F1", 10, 125, y, b);
    }
    let out = convert(&make_pdf(&[&page], "[0 0 300 300]", None));
    assert!(out.contains("| Name | Age |"), "{out}");
    assert!(out.contains("| --- | --- |"), "{out}");
    assert!(out.contains("| Alice | 30 |"), "{out}");
    assert!(out.contains("| Bob | 41 |"), "{out}");
}

#[test]
fn test_borderless_table_from_aligned_columns() {
    let rows = [
        ("Item", "Qty", "Price"),
        ("Apple", "3", "1.20"),
        ("Pear", "5", "2.10"),
        ("Plum", "7", "0.90"),
    ];
    let mut page = String::new();
    for (i, (a, b, c)) in rows.iter().enumerate() {
        let y = 250 - i as u32 * 14;
        page += &text("F1", 10, 20, y, a);
        page += &text("F1", 10, 120, y, b);
        page += &text("F1", 10, 200, y, c);
    }
    let out = convert(&make_pdf(&[&page], "[0 0 300 300]", None));
    assert!(out.contains("| Item | Qty | Price |"), "{out}");
    assert!(out.contains("| Apple | 3 | 1.20 |"), "{out}");
}

#[test]
fn test_bullet_list() {
    let page = text("F1", 10, 20, 250, "\\225")
        + &text("F1", 10, 34, 250, "First item")
        + &text("F1", 10, 20, 238, "\\225")
        + &text("F1", 10, 34, 238, "Second item");
    let out = convert(&make_pdf(&[&page], "[0 0 300 300]", None));
    assert!(out.contains("- First item"), "{out}");
    assert!(out.contains("- Second item"), "{out}");
}

#[test]
fn test_bold_run_is_emphasised() {
    let page = text("F1", 10, 20, 250, "Plain words then")
        + &text("F2", 10, 112, 250, "bold words")
        + &text("F1", 10, 168, 250, "and plain again");
    let out = convert(&make_pdf(&[&page], "[0 0 300 300]", None));
    assert!(out.contains("**bold words**"), "{out}");
}

#[test]
fn test_pages_are_independent_threads() {
    let pages: Vec<String> = (0..12)
        .map(|i| text("F1", 10, 20, 150, &format!("Unique body text number {i}.")))
        .collect();
    let refs: Vec<&str> = pages.iter().map(String::as_str).collect();
    let out = convert(&make_pdf(&refs, "[0 0 300 300]", None));
    let mut last = 0;
    for i in 0..12 {
        let pos = out.find(&format!("number {i}.")).expect(&out);
        assert!(pos >= last, "pages out of order");
        last = pos;
    }
}

#[test]
fn test_rotated_page_reads_in_displayed_order() {
    // Text runs upward in user space; /Rotate 90 turns it into normal
    // left-to-right lines. Larger user x ends up lower on the displayed page.
    let line = |x: u32, s: &str| format!("BT /F1 10 Tf 0 1 -1 0 {x} 20 Tm ({s}) Tj ET ");
    let page = line(120, "Second displayed line of text") + &line(100, "First displayed line of text");
    let out = convert(&make_pdf(&[&page], "[0 0 300 200] /Rotate 90", None));
    let first = out.find("First displayed line").expect(&out);
    let second = out.find("Second displayed line").expect(&out);
    assert!(first < second, "{out}");
    assert!(out.contains("First displayed line of text"), "{out}");
}

#[test]
fn test_form_xobject_text_is_extracted() {
    // Text drawn from a Form XObject with its own matrix.
    let mut pdf = String::from("%PDF-1.4\n");
    let objs = [
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Resources << /XObject << /X1 5 0 R >> /Font << /F1 6 0 R >> >> /Contents 4 0 R >>".to_string(),
        ),
        (4, "<< /Length 8 >>\nstream\n/X1 Do\nendstream".to_string()),
        (
            5,
            "<< /Type /XObject /Subtype /Form /BBox [0 0 300 300] /Matrix [1 0 0 1 10 100] /Resources << /Font << /F1 6 0 R >> >> /Length 55 >>\nstream\nBT /F1 10 Tf 20 20 Td (Inside a form object) Tj ET\nendstream".to_string(),
        ),
        (6, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string()),
    ];
    let mut offsets = vec![0usize; 7];
    for (n, body) in &objs {
        offsets[*n] = pdf.len();
        pdf.push_str(&format!("{n} 0 obj\n{body}\nendobj\n"));
    }
    let xref = pdf.len();
    pdf.push_str("xref\n0 7\n0000000000 65535 f \n");
    for o in &offsets[1..] {
        pdf.push_str(&format!("{o:010} 00000 n \n"));
    }
    pdf.push_str(&format!("trailer\n<< /Size 7 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF"));
    let out = convert(pdf.as_bytes());
    assert!(out.contains("Inside a form object"), "{out}");
}

#[test]
fn test_link_annotation_becomes_markdown_link() {
    let page = text("F1", 10, 20, 250, "Visit the project site today");
    let mut pdf = String::from_utf8(make_pdf(&[&page], "[0 0 300 300]", None)).unwrap();
    // Attach a link annotation over the text by rewriting the page object.
    pdf = pdf.replace(
        "/MediaBox [0 0 300 300] /Contents 4 0 R",
        "/MediaBox [0 0 300 300] /Contents 4 0 R /Annots [<< /Type /Annot /Subtype /Link /Rect [18 245 150 262] /A << /S /URI /URI (https://example.com/) >> >>]",
    );
    let out = convert(pdf.as_bytes());
    assert!(out.contains("](https://example.com/)"), "{out}");
}

fn three_pages() -> Vec<u8> {
    let page = |body: &str| text("F1", 10, 20, 220, body);
    let (a, b, c) = (
        page("Alpha findings."),
        page("Bravo findings."),
        page("Charlie findings."),
    );
    make_pdf(&[&a, &b, &c], "[0 0 300 300]", None)
}

fn convert_pages(input: &[u8], pages: &str) -> Result<String> {
    let options = ConvertOptions {
        pages: Some(pages.parse().unwrap()),
        ..Default::default()
    };
    let mut out = Vec::new();
    PdfConverter.convert_with(input, &mut out, &options)?;
    Ok(String::from_utf8(out).unwrap())
}

#[test]
fn test_pages_option_selects_pages_and_keeps_numbers() {
    let pdf = three_pages();
    let out = convert_pages(&pdf, "2").unwrap();
    assert!(out.contains("<!-- page 2 -->") && out.contains("Bravo findings."), "{out}");
    assert!(!out.contains("page 1") && !out.contains("Alpha"), "{out}");
    assert!(!out.contains("page 3") && !out.contains("Charlie"), "{out}");

    let out = convert_pages(&pdf, "1,3").unwrap();
    assert!(out.contains("Alpha") && out.contains("Charlie") && !out.contains("Bravo"), "{out}");
    assert!(out.contains("<!-- page 3 -->"), "{out}");
}

#[test]
fn test_selected_page_matches_the_full_conversion() {
    let page = |body: &str| text("F1", 10, 20, 220, body);
    let big = text("F1", 24, 20, 250, "Chapter Title") + &page("Body text of page two.");
    let (a, b) = (page("Intro paragraph one."), big);
    let pdf = make_pdf(&[&a, &b], "[0 0 300 300]", None);

    let full = convert(&pdf);
    let only = convert_pages(&pdf, "2").unwrap();
    let section = |s: &str| s[s.find("<!-- page 2 -->").unwrap()..].to_string();
    assert_eq!(section(&full), section(&only));
}

#[test]
fn test_blank_selected_page_keeps_its_marker() {
    let p1 = text("F1", 10, 20, 220, "Page one content.");
    let pdf = make_pdf(&[&p1, ""], "[0 0 300 300]", None);

    let out = convert_pages(&pdf, "2").unwrap();
    assert!(out.contains("<!-- page 2 -->"), "{out}");
    assert!(!out.contains("no extractable text") && !out.contains("page 1"), "{out}");

    // Without `--pages` the document-level fallback is unchanged.
    let blank = make_pdf(&["", ""], "[0 0 300 300]", None);
    assert!(convert(&blank).contains("no extractable text"));
}

#[test]
fn test_pages_option_outside_the_document_is_an_error() {
    let pdf = three_pages();
    let err = convert_pages(&pdf, "9-").unwrap_err().to_string();
    assert!(err.contains("no page matches"), "{err}");
}
