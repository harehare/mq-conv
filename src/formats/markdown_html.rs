use std::io::Write;

use mq_markdown::Markdown;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct MarkdownHtmlConverter;

impl Converter for MarkdownHtmlConverter {
    fn format_name(&self) -> &'static str {
        "markdown-html"
    }

    fn output_extension(&self) -> &'static str {
        "html"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let markdown = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "markdown-html",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        let parsed = markdown.parse::<Markdown>().map_err(|e| Error::Conversion {
            format: "markdown-html",
            message: e.to_string(),
        })?;

        let html = parsed.to_html();
        writer.write_all(html.as_bytes())?;
        Ok(())
    }
}
