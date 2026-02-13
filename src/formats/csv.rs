use std::io::Write;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct CsvConverter;

impl Converter for CsvConverter {
    fn format_name(&self) -> &'static str {
        "csv"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(input);

        let headers = reader.headers().map_err(|e| Error::Conversion {
            format: "csv",
            message: e.to_string(),
        })?;

        let col_count = headers.len();
        if col_count == 0 {
            writeln!(writer, "*Empty CSV*")?;
            return Ok(());
        }

        // Header row
        write!(writer, "|")?;
        for field in headers.iter() {
            write!(writer, " {} |", escape_pipe(field))?;
        }
        writeln!(writer)?;

        // Separator
        write!(writer, "|")?;
        for _ in 0..col_count {
            write!(writer, "---|")?;
        }
        writeln!(writer)?;

        // Data rows
        for result in reader.records() {
            let record = result.map_err(|e| Error::Conversion {
                format: "csv",
                message: e.to_string(),
            })?;
            write!(writer, "|")?;
            for i in 0..col_count {
                let cell = record.get(i).unwrap_or("");
                write!(writer, " {} |", escape_pipe(cell))?;
            }
            writeln!(writer)?;
        }

        Ok(())
    }
}

fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|")
}
