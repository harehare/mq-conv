use crate::error::Result;
use std::io::Write;
use std::path::PathBuf;

/// Options shared by converters that support them; converters ignore the
/// fields they do not use.
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// Write embedded images into this directory and reference them from the
    /// Markdown output.
    pub image_dir: Option<PathBuf>,
    /// Tesseract language used for OCR fallbacks (default "eng").
    pub ocr_lang: Option<String>,
}

pub trait Converter {
    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()>;

    /// Like [`Converter::convert`] but honouring [`ConvertOptions`].
    fn convert_with(
        &self,
        input: &[u8],
        writer: &mut dyn Write,
        _options: &ConvertOptions,
    ) -> Result<()> {
        self.convert(input, writer)
    }

    fn format_name(&self) -> &'static str;
    fn output_extension(&self) -> &'static str {
        "md"
    }
}
