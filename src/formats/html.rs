use std::io::Write;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct HtmlConverter;

impl Converter for HtmlConverter {
    fn format_name(&self) -> &'static str {
        "html"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let text = mq_markdown::convert_html_to_markdown(
            std::str::from_utf8(input).map_err(|e| Error::Conversion {
                format: "html",
                message: e.to_string(),
            })?,
            mq_markdown::ConversionOptions {
                extract_scripts_as_code_blocks: true,
                generate_front_matter: true,
                use_title_as_h1: true,
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
