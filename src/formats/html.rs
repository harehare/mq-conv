use std::io::Write;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct HtmlConverter;

impl Converter for HtmlConverter {
    fn format_name(&self) -> &'static str {
        "html"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let decoded = crate::formats::encoding::decode_html(input);
        let text = mq_markdown::convert_html_to_markdown(
            &decoded,
            mq_markdown::ConversionOptions {
                extract_scripts_as_code_blocks: true,
                generate_front_matter: true,
                use_title_as_h1: true,
                base_url: None,
            },
        )
        .map_err(|e| Error::Conversion {
            format: "html",
            message: e.to_string(),
        })?;

        let trimmed = text.trim();
        if trimmed.is_empty() {
            writeln!(writer, "*Empty HTML document*")?;
        } else {
            writeln!(writer, "{trimmed}")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(input: &[u8]) -> String {
        let converter = HtmlConverter;
        let mut output = Vec::new();
        converter.convert(input, &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn converts_utf8_japanese_html() {
        let output = convert("<h1>日本語の見出し</h1><p>本文です</p>".as_bytes());
        assert!(output.contains("日本語の見出し"), "{output}");
        assert!(output.contains("本文です"), "{output}");
    }

    #[test]
    fn converts_shift_jis_html_declared_via_meta_charset() {
        let (body, _, had_errors) = encoding_rs::SHIFT_JIS
            .encode(r#"<html><head><meta charset="Shift_JIS"></head><body><h1>日本語ページ</h1></body></html>"#);
        assert!(!had_errors);
        let output = convert(&body);
        assert!(output.contains("日本語ページ"), "{output}");
    }
}
