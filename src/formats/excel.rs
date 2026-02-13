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
            writeln!(writer)?;

            let rows: Vec<Vec<String>> = range
                .rows()
                .map(|row| row.iter().map(format_cell).collect())
                .collect();

            if rows.is_empty() {
                writeln!(writer, "*Empty sheet*")?;
                continue;
            }

            let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
            if col_count == 0 {
                writeln!(writer, "*Empty sheet*")?;
                continue;
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
        }

        Ok(())
    }
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
