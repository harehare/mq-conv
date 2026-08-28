use std::io::Write;

use leptess::LepTess;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct OcrConverter {
    pub lang: String,
}

impl Default for OcrConverter {
    fn default() -> Self {
        Self {
            lang: "eng".to_string(),
        }
    }
}

impl Converter for OcrConverter {
    fn format_name(&self) -> &'static str {
        "ocr"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let mut lt = LepTess::new(None, &self.lang).map_err(|e| Error::Conversion {
            format: "ocr",
            message: format!(
                "Failed to initialize Tesseract for language \"{}\" (is tesseract, and its \"{}\" language pack, installed?): {e}",
                self.lang, self.lang
            ),
        })?;

        lt.set_image_from_mem(input).map_err(|e| Error::Conversion {
            format: "ocr",
            message: format!("Failed to load image for OCR: {e}"),
        })?;

        let text = lt.get_utf8_text().map_err(|e| Error::Conversion {
            format: "ocr",
            message: format!("OCR extraction failed: {e}"),
        })?;

        for line in text.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                writeln!(writer, "{trimmed}")?;
            } else {
                writeln!(writer)?;
            }
        }

        Ok(())
    }
}
