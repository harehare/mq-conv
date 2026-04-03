use std::io::Write;

use mq_markdown::Markdown;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct MarkdownJsonAstConverter;

impl Converter for MarkdownJsonAstConverter {
    fn format_name(&self) -> &'static str {
        "markdown-json-ast"
    }

    fn output_extension(&self) -> &'static str {
        "json"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let markdown = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "markdown-json-ast",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        let parsed = markdown.parse::<Markdown>().map_err(|e| Error::Conversion {
            format: "markdown-json-ast",
            message: e.to_string(),
        })?;

        let json = parsed.to_json().map_err(|e| Error::Conversion {
            format: "markdown-json-ast",
            message: e.to_string(),
        })?;

        writer.write_all(json.as_bytes())?;
        writeln!(writer)?;
        Ok(())
    }
}
