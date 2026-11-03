use std::io::Write;

use crate::ast;
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

        if headers.is_empty() {
            return ast::write(writer, vec![ast::emphasis("Empty CSV")]);
        }

        let mut rows = vec![headers.iter().map(str::to_owned).collect::<Vec<_>>()];
        for result in reader.records() {
            let record = result.map_err(|e| Error::Conversion {
                format: "csv",
                message: e.to_string(),
            })?;
            rows.push(record.iter().map(str::to_owned).collect());
        }

        ast::write(writer, ast::table(&rows))
    }
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
        assert!(output.contains("| 田中 | 30   |"), "{output}");
    }

    #[test]
    fn converts_shift_jis_csv_exported_from_excel() {
        let (bytes, _, had_errors) = encoding_rs::SHIFT_JIS.encode("名前,部署\n山田太郎,営業部\n");
        assert!(!had_errors);
        let output = convert(&bytes);
        assert!(output.contains("| 名前     | 部署   |"), "{output}");
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
