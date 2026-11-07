use std::io::{Cursor, Write};

use calamine::{Data, Range, Reader, SheetVisible, open_workbook_auto_from_rs};

use crate::converter::{ConvertOptions, Converter};
use crate::document::{Block as DocBlock, Document, Inline};
use crate::error::{Error, Result};

pub struct ExcelConverter;

impl Converter for ExcelConverter {
    fn format_name(&self) -> &'static str {
        "excel"
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
        let cursor = Cursor::new(input);
        let mut workbook = open_workbook_auto_from_rs(cursor).map_err(|e| Error::Conversion {
            format: "excel",
            message: e.to_string(),
        })?;

        let sheet_names: Vec<String> = workbook.sheet_names().to_vec();
        options.check_pages("excel", "sheet", sheet_names.len())?;

        let mut doc = Document::new();
        for (idx, name) in sheet_names.iter().enumerate() {
            // Sheets are numbered by position; unselected ones are never read.
            if !options.is_selected(idx as u32 + 1) {
                continue;
            }
            let range = workbook
                .worksheet_range(name)
                .map_err(|e| Error::Conversion {
                    format: "excel",
                    message: e.to_string(),
                })?;

            doc.push(DocBlock::heading(1, name));
            match workbook.sheets_metadata().get(idx).map(|s| s.visible) {
                Some(SheetVisible::Hidden) => doc.push(DocBlock::emphasized("Hidden sheet")),
                Some(SheetVisible::VeryHidden) => {
                    doc.push(DocBlock::emphasized("Very hidden sheet"));
                }
                _ => {}
            }

            let rows: Vec<Vec<String>> = range
                .rows()
                .map(|row| row.iter().map(format_cell).collect())
                .collect();

            let blocks = split_into_blocks(rows);
            if blocks.is_empty() {
                doc.push(DocBlock::emphasized("Empty sheet"));
                continue;
            }
            let formulas = if options.formulas {
                workbook.worksheet_formula(name).ok()
            } else {
                None
            };

            for block in blocks {
                match classify_block(block) {
                    Block::Table(rows) => doc.push(DocBlock::text_table(&rows)),
                    Block::Text(lines) => {
                        for line in lines.into_iter().filter(|l| !l.is_empty()) {
                            doc.push(DocBlock::paragraph(line));
                        }
                    }
                }
            }
            if let Some(table) = formulas.and_then(|f| formula_table(&f, &range)) {
                doc.push(DocBlock::Paragraph(vec![Inline::Strong(vec![
                    Inline::Text("Formulas".to_string()),
                ])]));
                doc.push(table);
            }
        }

        doc.write_markdown(writer)
    }
}

/// The cells that hold a formula as a `Cell | Formula | Value` table.
fn formula_table(formulas: &Range<String>, values: &Range<Data>) -> Option<DocBlock> {
    let (row0, col0) = formulas.start()?;
    let mut rows = vec![
        ["Cell", "Formula", "Value"]
            .map(|h| vec![Inline::Text(h.to_string())])
            .to_vec(),
    ];
    for (row, col, formula) in formulas.cells() {
        if formula.is_empty() {
            continue;
        }
        let at = (row0 + row as u32, col0 + col as u32);
        let value = values.get_value(at).map(format_cell).unwrap_or_default();
        rows.push(vec![
            vec![Inline::Text(cell_address(at))],
            vec![Inline::Code(format!(
                "={}",
                formula.trim_start_matches('=')
            ))],
            vec![Inline::Text(value)],
        ]);
    }
    (rows.len() > 1).then_some(DocBlock::Table(rows))
}

/// `(0, 0)` is `A1`, `(2, 27)` is `AB3`.
fn cell_address((row, col): (u32, u32)) -> String {
    let mut letters = Vec::new();
    let mut n = col + 1;
    while n > 0 {
        letters.push(b'A' + ((n - 1) % 26) as u8);
        n = (n - 1) / 26;
    }
    letters.reverse();
    format!("{}{}", String::from_utf8_lossy(&letters), row + 1)
}

enum Block {
    Table(Vec<Vec<String>>),
    Text(Vec<String>),
}

fn split_into_blocks(rows: Vec<Vec<String>>) -> Vec<Vec<Vec<String>>> {
    let mut blocks = Vec::new();
    let mut current: Vec<Vec<String>> = Vec::new();

    for row in rows {
        if is_blank_row(&row) {
            if !current.is_empty() {
                blocks.push(current);
                current = Vec::new();
            }
        } else {
            current.push(row);
        }
    }

    if !current.is_empty() {
        blocks.push(current);
    }

    blocks
}

fn classify_block(block: Vec<Vec<String>>) -> Block {
    if block.len() >= 2 {
        let multi_col_rows = block
            .iter()
            .filter(|row| row.iter().filter(|c| !c.is_empty()).count() >= 2)
            .count();

        if multi_col_rows * 2 > block.len() {
            return Block::Table(block);
        }
    }

    let lines = block
        .into_iter()
        .map(|row| {
            row.into_iter()
                .filter(|c| !c.is_empty())
                .collect::<Vec<_>>()
                .join("  ")
        })
        .collect();

    Block::Text(lines)
}

fn is_blank_row(row: &[String]) -> bool {
    row.iter().all(|c| c.is_empty())
}

fn format_cell(data: &Data) -> String {
    match data {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Int(n) => n.to_string(),
        Data::Float(f) => {
            if *f == f.trunc() {
                format!("{f:.0}")
            } else {
                f.to_string()
            }
        }
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => {
            let (y, mo, d, h, m, s, _) = dt.to_ymd_hms_milli();
            if dt.is_duration() {
                format!("{h:02}:{m:02}:{s:02}")
            } else if h == 0 && m == 0 && s == 0 {
                format!("{y:04}-{mo:02}-{d:02}")
            } else {
                format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}")
            }
        }
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::{ConvertOptions, Converter};
    use rstest::rstest;

    // ── unit tests ────────────────────────────────────────────────────────────

    #[test]
    fn test_cell_address() {
        assert_eq!(cell_address((0, 0)), "A1");
        assert_eq!(cell_address((2, 25)), "Z3");
        assert_eq!(cell_address((2, 26)), "AA3");
        assert_eq!(cell_address((0, 27)), "AB1");
    }

    #[rstest]
    #[case(vec![], true)]
    #[case(vec!["".to_string()], true)]
    #[case(vec!["".to_string(), "".to_string()], true)]
    #[case(vec!["a".to_string()], false)]
    #[case(vec!["".to_string(), "b".to_string()], false)]
    fn test_is_blank_row(#[case] row: Vec<String>, #[case] expected: bool) {
        assert_eq!(is_blank_row(&row), expected);
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_split_empty() {
        assert!(split_into_blocks(vec![]).is_empty());
    }

    #[test]
    fn test_split_no_blank_rows() {
        let rows = vec![s(&["a", "b"]), s(&["c", "d"])];
        let blocks = split_into_blocks(rows.clone());
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], rows);
    }

    #[test]
    fn test_split_blank_row_separator() {
        let rows = vec![
            s(&["title"]),
            s(&[""]),
            s(&["Name", "Age"]),
            s(&["Alice", "30"]),
        ];
        let blocks = split_into_blocks(rows);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], vec![s(&["title"])]);
        assert_eq!(blocks[1], vec![s(&["Name", "Age"]), s(&["Alice", "30"])]);
    }

    #[test]
    fn test_split_multiple_blank_rows_collapsed() {
        let rows = vec![s(&["a"]), s(&[""]), s(&[""]), s(&["b"])];
        let blocks = split_into_blocks(rows);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn test_split_leading_trailing_blank_rows_ignored() {
        let rows = vec![s(&[""]), s(&["a", "b"]), s(&["c", "d"]), s(&[""])];
        let blocks = split_into_blocks(rows);
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn test_classify_dense_multi_column_is_table() {
        let block = vec![
            s(&["Name", "Age", "City"]),
            s(&["Alice", "30", "Tokyo"]),
            s(&["Bob", "25", "Osaka"]),
        ];
        assert!(matches!(classify_block(block), Block::Table(_)));
    }

    #[test]
    fn test_classify_single_row_is_text() {
        let block = vec![s(&["Report Title"])];
        assert!(matches!(classify_block(block), Block::Text(_)));
    }

    #[test]
    fn test_classify_single_column_multi_row_is_text() {
        let block = vec![s(&["Line one"]), s(&["Line two"]), s(&["Line three"])];
        assert!(matches!(classify_block(block), Block::Text(_)));
    }

    #[test]
    fn test_classify_sparse_rows_is_text() {
        // Only 1 out of 3 rows has 2+ cells — does not reach majority threshold
        let block = vec![s(&["Label", "Value"]), s(&["Note"]), s(&["Footer"])];
        assert!(matches!(classify_block(block), Block::Text(_)));
    }

    // ── integration tests (require zip feature to build xlsx) ─────────────────

    #[cfg(feature = "zip")]
    mod xlsx_tests {
        use super::*;
        use std::io::Write;

        /// Build a minimal xlsx from a 2-D grid of strings.
        /// Empty rows in `rows` (empty slices `&[]`) become gaps in row numbering
        /// so calamine produces blank rows in the Range.
        fn make_xlsx(sheet_name: &str, rows: &[&[&str]]) -> Vec<u8> {
            fn col_letter(i: usize) -> char {
                (b'A' + i as u8) as char
            }

            let mut sheet_data = String::new();
            for (r, row) in rows.iter().enumerate() {
                if row.is_empty() {
                    continue; // gap → calamine fills with empty row
                }
                let row_num = r + 1;
                sheet_data.push_str(&format!("<row r=\"{row_num}\">"));
                for (c, cell) in row.iter().enumerate() {
                    if cell.is_empty() {
                        continue;
                    }
                    let addr = format!("{}{}", col_letter(c), row_num);
                    sheet_data.push_str(&format!(
                        "<c r=\"{addr}\" t=\"inlineStr\"><is><t>{cell}</t></is></c>"
                    ));
                }
                sheet_data.push_str("</row>");
            }

            let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
</Types>"#;

            let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

            let workbook = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
          xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets><sheet name="{sheet_name}" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#
            );

            let workbook_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
</Relationships>"#;

            let worksheet = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>{sheet_data}</sheetData>
</worksheet>"#
            );

            let buf = Vec::new();
            let cursor = std::io::Cursor::new(buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            for (name, content) in [
                ("[Content_Types].xml", content_types.to_string()),
                ("_rels/.rels", rels.to_string()),
                ("xl/workbook.xml", workbook),
                ("xl/_rels/workbook.xml.rels", workbook_rels.to_string()),
                ("xl/worksheets/sheet1.xml", worksheet),
            ] {
                zip.start_file(name, opts).unwrap();
                zip.write_all(content.as_bytes()).unwrap();
            }

            zip.finish().unwrap().into_inner()
        }

        /// A workbook with one sheet per `(name, text)`, the text in cell A1.
        fn make_multi_sheet_xlsx(sheets: &[(&str, &str)]) -> Vec<u8> {
            let n = sheets.len();
            let overrides: String = (1..=n)
                .map(|i| format!(r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#))
                .collect();
            let content_types = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>{overrides}
</Types>"#
            );
            let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;
            let sheet_tags: String = sheets
                .iter()
                .enumerate()
                .map(|(i, (name, _))| {
                    format!(
                        r#"<sheet name="{name}" sheetId="{0}" r:id="rId{0}"/>"#,
                        i + 1
                    )
                })
                .collect();
            let workbook = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
          xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets>{sheet_tags}</sheets>
</workbook>"#
            );
            let rel_tags: String = (1..=n)
                .map(|i| format!(r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#))
                .collect();
            let workbook_rels = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{rel_tags}</Relationships>"#
            );

            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            let mut files = vec![
                ("[Content_Types].xml".to_string(), content_types),
                ("_rels/.rels".to_string(), rels.to_string()),
                ("xl/workbook.xml".to_string(), workbook),
                ("xl/_rels/workbook.xml.rels".to_string(), workbook_rels),
            ];
            for (i, (_, text)) in sheets.iter().enumerate() {
                files.push((
                    format!("xl/worksheets/sheet{}.xml", i + 1),
                    format!(
                        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{text}</t></is></c></row></sheetData></worksheet>"#
                    ),
                ));
            }
            for (name, content) in files {
                zip.start_file(name, opts).unwrap();
                zip.write_all(content.as_bytes()).unwrap();
            }
            zip.finish().unwrap().into_inner()
        }

        fn convert_pages(xlsx: &[u8], pages: &str) -> Result<String> {
            let options = ConvertOptions {
                pages: Some(pages.parse().unwrap()),
                ..Default::default()
            };
            let mut out = Vec::new();
            ExcelConverter.convert_with(xlsx, &mut out, &options)?;
            Ok(String::from_utf8(out).unwrap())
        }

        #[test]
        fn pages_select_sheets_by_position() {
            let xlsx = make_multi_sheet_xlsx(&[
                ("Alpha", "a-text"),
                ("Beta", "b-text"),
                ("Gamma", "c-text"),
            ]);

            let out = convert_pages(&xlsx, "2").unwrap();
            assert!(out.starts_with("# Beta\n"), "{out}");
            assert!(
                out.contains("b-text") && !out.contains("a-text") && !out.contains("c-text"),
                "{out}"
            );

            let out = convert_pages(&xlsx, "1,3").unwrap();
            assert!(
                out.contains("# Alpha") && out.contains("# Gamma") && !out.contains("Beta"),
                "{out}"
            );
            assert!(
                out.contains("a-text\n\n# Gamma"),
                "sheets not separated:\n{out}"
            );

            // Without a selection every sheet is converted, as before.
            let all = ExcelConverter.convert(&xlsx, &mut Vec::new());
            assert!(all.is_ok());
        }

        #[test]
        fn pages_outside_the_workbook_are_an_error() {
            let xlsx = make_multi_sheet_xlsx(&[("Alpha", "a-text")]);
            let err = convert_pages(&xlsx, "2-3").unwrap_err().to_string();
            assert!(err.contains("no sheet matches"), "{err}");
        }

        fn make_xlsx_with_date_cell(serial: &str) -> Vec<u8> {
            let content_types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
  <Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
  <Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
</Types>"#;

            let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

            let workbook = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"
          xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <sheets><sheet name="Dates" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#
                .to_string();

            let workbook_rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#;

            let styles = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <numFmts count="1"><numFmt numFmtId="164" formatCode="yyyy-mm-dd"/></numFmts>
  <fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>
  <fills count="1"><fill><patternFill patternType="none"/></fill></fills>
  <borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
  <cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
  <cellXfs count="2">
    <xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>
    <xf numFmtId="164" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/>
  </cellXfs>
</styleSheet>"#;

            let worksheet = format!(
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData><row r="1"><c r="A1" s="1"><v>{serial}</v></c></row></sheetData>
</worksheet>"#
            );

            let buf = Vec::new();
            let cursor = std::io::Cursor::new(buf);
            let mut zip = zip::ZipWriter::new(cursor);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);

            for (name, content) in [
                ("[Content_Types].xml", content_types.to_string()),
                ("_rels/.rels", rels.to_string()),
                ("xl/workbook.xml", workbook),
                ("xl/_rels/workbook.xml.rels", workbook_rels.to_string()),
                ("xl/styles.xml", styles.to_string()),
                ("xl/worksheets/sheet1.xml", worksheet),
            ] {
                zip.start_file(name, opts).unwrap();
                zip.write_all(content.as_bytes()).unwrap();
            }

            zip.finish().unwrap().into_inner()
        }

        #[test]
        fn test_date_cell_renders_as_iso_date_not_serial_number() {
            let xlsx = make_xlsx_with_date_cell("44927");
            let out = convert(&xlsx);
            assert!(out.contains("2023-01-01"), "expected ISO date in:\n{out}");
            assert!(
                !out.contains("44927"),
                "raw Excel serial leaked through:\n{out}"
            );
        }

        fn convert(data: &[u8]) -> String {
            let mut out = Vec::new();
            ExcelConverter.convert(data, &mut out).unwrap();
            String::from_utf8(out).unwrap()
        }

        #[test]
        fn test_pure_table() {
            let xlsx = make_xlsx(
                "Sales",
                &[
                    &["Name", "Age", "City"],
                    &["Alice", "30", "Tokyo"],
                    &["Bob", "25", "Osaka"],
                ],
            );
            let out = convert(&xlsx);
            assert!(out.contains("# Sales"), "sheet heading missing");
            assert!(
                out.contains("| Name  | Age | City  |"),
                "header row missing"
            );
            assert!(out.contains("| ----- | --- | ----- |"), "separator missing");
            assert!(out.contains("| Alice | 30  | Tokyo |"), "data row missing");
            assert!(out.contains("| Bob   | 25  | Osaka |"), "data row missing");
        }

        #[test]
        fn test_text_only_single_column() {
            let xlsx = make_xlsx(
                "Notes",
                &[&["First note"], &["Second note"], &["Third note"]],
            );
            let out = convert(&xlsx);
            assert!(out.contains("First note"), "text line missing");
            assert!(out.contains("Second note"), "text line missing");
            assert!(!out.contains('|'), "should not be a table");
        }

        #[test]
        fn test_mixed_title_blank_table() {
            let xlsx = make_xlsx(
                "Report",
                &[
                    &["Monthly Report"],
                    &[], // blank row
                    &["Name", "Score"],
                    &["Alice", "95"],
                    &["Bob", "87"],
                ],
            );
            let out = convert(&xlsx);
            assert!(out.contains("Monthly Report"), "title missing");
            assert!(out.contains("| Name  | Score |"), "table header missing");
            assert!(out.contains("| Alice | 95    |"), "table row missing");
            // title should NOT appear as a table row
            assert!(
                !out.contains("| Monthly Report |"),
                "title rendered as table row"
            );
        }

        #[test]
        fn test_mixed_table_blank_note() {
            let xlsx = make_xlsx(
                "Sheet1",
                &[
                    &["Item", "Qty"],
                    &["Apple", "10"],
                    &[], // blank row
                    &["Note: draft only"],
                ],
            );
            let out = convert(&xlsx);
            assert!(out.contains("| Item  | Qty |"), "table missing");
            assert!(out.contains("Note: draft only"), "note missing");
            assert!(
                !out.contains("| Note: draft only |"),
                "note rendered as table row"
            );
        }

        #[test]
        fn test_pipe_escaped_in_cell() {
            let xlsx = make_xlsx("S", &[&["a|b", "c"], &["x|y", "z"]]);
            let out = convert(&xlsx);
            assert!(out.contains("a\\|b"), "pipe not escaped");
        }

        #[test]
        fn test_multiline_cell_kept_on_one_table_row() {
            let xlsx = make_xlsx("S", &[&["Name", "Note"], &["Alice", "Line one\nLine two"]]);
            let out = convert(&xlsx);
            assert!(out.contains("| Alice | Line one<br>Line two |"), "{out}");
        }

        #[test]
        fn test_sheet_name_as_heading() {
            let xlsx = make_xlsx("MySheet", &[&["a", "b"], &["1", "2"]]);
            let out = convert(&xlsx);
            assert!(out.starts_with("# MySheet\n"), "sheet heading wrong");
        }

        /// A one-sheet workbook with the given `<sheet>` attributes and sheet XML.
        fn make_xlsx_raw(sheet_attrs: &str, sheet_xml: &str) -> Vec<u8> {
            let parts = [
                ("[Content_Types].xml", r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_string()),
                ("_rels/.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string()),
                ("xl/workbook.xml", format!(r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Data" sheetId="1" {sheet_attrs} r:id="rId1"/></sheets></workbook>"#)),
                ("xl/_rels/workbook.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_string()),
                ("xl/worksheets/sheet1.xml", format!(r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{sheet_xml}</sheetData></worksheet>"#)),
            ];
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, content) in parts {
                zip.start_file(name, opts).unwrap();
                zip.write_all(content.as_bytes()).unwrap();
            }
            zip.finish().unwrap().into_inner()
        }

        const FORMULA_ROWS: &str = r#"<row r="1"><c r="A1"><v>1</v></c><c r="B1"><v>2</v></c></row><row r="2"><c r="A2"><v>3</v></c><c r="B2"><f>A1+A2</f><v>4</v></c></row>"#;

        #[test]
        fn formulas_are_listed_only_on_request() {
            let xlsx = make_xlsx_raw("", FORMULA_ROWS);
            let mut plain = Vec::new();
            ExcelConverter.convert(&xlsx, &mut plain).unwrap();
            assert!(!String::from_utf8(plain).unwrap().contains("Formulas"));

            let options = ConvertOptions {
                formulas: true,
                ..Default::default()
            };
            let mut out = Vec::new();
            ExcelConverter
                .convert_with(&xlsx, &mut out, &options)
                .unwrap();
            let md = String::from_utf8(out).unwrap();
            assert!(md.contains("**Formulas**"), "{md}");
            assert!(md.contains("| B2 "), "{md}");
            assert!(md.contains("`=A1+A2`"), "{md}");
            assert!(md.contains("| 4 "), "{md}");
        }

        #[test]
        fn hidden_sheets_are_marked() {
            let xlsx = make_xlsx_raw(r#"state="hidden""#, FORMULA_ROWS);
            let mut out = Vec::new();
            ExcelConverter.convert(&xlsx, &mut out).unwrap();
            let md = String::from_utf8(out).unwrap();
            assert!(md.starts_with("# Data\n\n*Hidden sheet*\n"), "{md}");
        }
    }
}
