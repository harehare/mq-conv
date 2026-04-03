use std::io::Write;

use mq_markdown::Markdown;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct MarkdownTextConverter;

impl Converter for MarkdownTextConverter {
    fn format_name(&self) -> &'static str {
        "markdown-text"
    }

    fn output_extension(&self) -> &'static str {
        "txt"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let markdown = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "markdown-text",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        let parsed = markdown.parse::<Markdown>().map_err(|e| Error::Conversion {
            format: "markdown-text",
            message: e.to_string(),
        })?;

        let text = parsed.to_text();
        writer.write_all(text.as_bytes())?;
        Ok(())
    }
}
