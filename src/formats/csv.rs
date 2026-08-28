use std::io::Write;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct CsvConverter;

impl Converter for CsvConverter {
    fn format_name(&self) -> &'static str {
        "csv"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let decoded = crate::formats::encoding::decode_text(input);
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true)
            .from_reader(decoded.as_bytes());

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
        .replace("\r\n", "<br>")
        .replace('\n', "<br>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(input: &[u8]) -> String {
        let converter = CsvConverter;
        let mut output = Vec::new();
        converter.convert(input, &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn converts_utf8_japanese_csv() {
        let output = convert("名前,年齢\n田中,30\n".as_bytes());
        assert!(output.contains("| 名前 | 年齢 |"), "{output}");
        assert!(output.contains("| 田中 | 30 |"), "{output}");
    }

    #[test]
    fn converts_shift_jis_csv_exported_from_excel() {
        let (bytes, _, had_errors) = encoding_rs::SHIFT_JIS.encode("名前,部署\n山田太郎,営業部\n");
        assert!(!had_errors);
        let output = convert(&bytes);
        assert!(output.contains("| 名前 | 部署 |"), "{output}");
        assert!(output.contains("| 山田太郎 | 営業部 |"), "{output}");
    }

    #[test]
    fn converts_quoted_multiline_field_without_breaking_table() {
        let output = convert(b"name,note\nAlice,\"Line one\nLine two\"\n");
        assert!(
            output.contains("| Alice | Line one<br>Line two |"),
            "{output}"
        );
        assert_eq!(output.lines().filter(|l| l.starts_with('|')).count(), 3);
    }
}
