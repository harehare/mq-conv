use std::io::{Cursor, Write};

use calamine::{Data, Reader, open_workbook_auto_from_rs};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct ExcelConverter;

impl Converter for ExcelConverter {
    fn format_name(&self) -> &'static str {
        "excel"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let cursor = Cursor::new(input);
        let mut workbook =
            open_workbook_auto_from_rs(cursor).map_err(|e| Error::Conversion {
                format: "excel",
                message: e.to_string(),
            })?;

        let sheet_names: Vec<String> = workbook.sheet_names().to_vec();

        for (idx, name) in sheet_names.iter().enumerate() {
            let range = workbook
                .worksheet_range(name)
                .map_err(|e| Error::Conversion {
                    format: "excel",
                    message: e.to_string(),
                })?;

            if idx > 0 {
                writeln!(writer)?;
            }
            writeln!(writer, "# {name}")?;

            let rows: Vec<Vec<String>> = range
                .rows()
                .map(|row| row.iter().map(format_cell).collect())
                .collect();

            if rows.is_empty() {
                writeln!(writer)?;
                writeln!(writer, "*Empty sheet*")?;
                continue;
            }

            let blocks = split_into_blocks(rows);
            if blocks.is_empty() {
                writeln!(writer)?;
                writeln!(writer, "*Empty sheet*")?;
                continue;
            }

            for block in blocks {
                writeln!(writer)?;
                match classify_block(block) {
                    Block::Table(rows) => write_table(writer, &rows)?,
                    Block::Text(lines) => write_text(writer, &lines)?,
                }
            }
        }

        Ok(())
    }
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

fn write_table(writer: &mut dyn Write, rows: &[Vec<String>]) -> Result<()> {
    let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if col_count == 0 {
        return Ok(());
    }

    // Header row
    let header = &rows[0];
    write!(writer, "|")?;
    for i in 0..col_count {
        let cell = header.get(i).map(|s| s.as_str()).unwrap_or("");
        write!(writer, " {cell} |")?;
    }
    writeln!(writer)?;

    // Separator
    write!(writer, "|")?;
    for _ in 0..col_count {
        write!(writer, "---|")?;
    }
    writeln!(writer)?;

    // Data rows
    for row in rows.iter().skip(1) {
        write!(writer, "|")?;
        for i in 0..col_count {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            write!(writer, " {cell} |")?;
        }
        writeln!(writer)?;
    }

    Ok(())
}

fn write_text(writer: &mut dyn Write, lines: &[String]) -> Result<()> {
    let mut first = true;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if !first {
            writeln!(writer)?;
        }
        writeln!(writer, "{line}")?;
        first = false;
    }
    Ok(())
}

fn is_blank_row(row: &[String]) -> bool {
    row.iter().all(|c| c.is_empty())
}

fn format_cell(data: &Data) -> String {
    match data {
        Data::Empty => String::new(),
        Data::String(s) => escape_pipe(s),
        Data::Int(n) => n.to_string(),
        Data::Float(f) => {
            if *f == f.trunc() {
                format!("{f:.0}")
            } else {
                f.to_string()
            }
        }
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => escape_pipe(&dt.to_string()),
        Data::DateTimeIso(s) => escape_pipe(s),
        Data::DurationIso(s) => escape_pipe(s),
        Data::Error(e) => format!("#{e:?}"),
    }
}

fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::Converter;
    use rstest::rstest;

    // ── unit tests ────────────────────────────────────────────────────────────

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
        let block = vec![
            s(&["Label", "Value"]),
            s(&["Note"]),
            s(&["Footer"]),
        ];
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
            assert!(out.contains("| Name | Age | City |"), "header row missing");
            assert!(out.contains("|---|---|---|"), "separator missing");
            assert!(out.contains("| Alice | 30 | Tokyo |"), "data row missing");
            assert!(out.contains("| Bob | 25 | Osaka |"), "data row missing");
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
            assert!(!out.contains("|---|"), "should not be a table");
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
            assert!(out.contains("| Name | Score |"), "table header missing");
            assert!(out.contains("| Alice | 95 |"), "table row missing");
            // title should NOT appear as a table row
            assert!(!out.contains("| Monthly Report |"), "title rendered as table row");
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
            assert!(out.contains("| Item | Qty |"), "table missing");
            assert!(out.contains("Note: draft only"), "note missing");
            assert!(!out.contains("| Note: draft only |"), "note rendered as table row");
        }

        #[test]
        fn test_pipe_escaped_in_cell() {
            let xlsx = make_xlsx("S", &[&["a|b", "c"], &["x|y", "z"]]);
            let out = convert(&xlsx);
            assert!(out.contains("a\\|b"), "pipe not escaped");
        }

        #[test]
        fn test_sheet_name_as_heading() {
            let xlsx = make_xlsx("MySheet", &[&["a", "b"], &["1", "2"]]);
            let out = convert(&xlsx);
            assert!(out.starts_with("# MySheet\n"), "sheet heading wrong");
        }
    }
}
