use std::io::{Cursor, Write};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct ImageConverter;

impl Converter for ImageConverter {
    fn format_name(&self) -> &'static str {
        "image"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        if is_svg(input) {
            writeln!(writer, "# Image")?;
            writeln!(writer)?;
            writeln!(writer, "| Property | Value |")?;
            writeln!(writer, "|----------|-------|")?;
            writeln!(writer, "| Format | SVG |")?;
            writeln!(writer, "| Size | {} |", format_size(input.len() as u64))?;
            return Ok(());
        }

        let cursor = Cursor::new(input);
        let reader = image::ImageReader::new(cursor)
            .with_guessed_format()
            .map_err(|e| Error::Conversion {
                format: "image",
                message: e.to_string(),
            })?;

        let format = reader.format();
        let img = reader.decode().map_err(|e| Error::Conversion {
            format: "image",
            message: e.to_string(),
        })?;

        writeln!(writer, "# Image")?;
        writeln!(writer)?;
        writeln!(writer, "| Property | Value |")?;
        writeln!(writer, "|----------|-------|")?;

        if let Some(fmt) = format {
            writeln!(writer, "| Format | {fmt:?} |")?;
        }

        writeln!(writer, "| Size | {} |", format_size(input.len() as u64))?;
        writeln!(
            writer,
            "| Dimensions | {}x{} |",
            img.width(),
            img.height()
        )?;
        writeln!(writer, "| Color Type | {:?} |", img.color())?;

        write_exif(input, writer)?;

        Ok(())
    }
}

fn write_exif(input: &[u8], writer: &mut dyn Write) -> Result<()> {
    let exif_reader = exif::Reader::new();
    let mut cursor = Cursor::new(input);
    let exif_data: exif::Exif = match exif_reader.read_from_container(&mut cursor) {
        Ok(exif) => exif,
        Err(_) => return Ok(()),
    };

    let fields: Vec<(String, String)> = exif_data
        .fields()
        .filter_map(|f| {
            let tag_name = f.tag.to_string();
            let value = f.display_value().with_unit(&exif_data).to_string();
            if value.is_empty() || value == "unknown" {
                return None;
            }
            Some((tag_name, value))
        })
        .collect();

    if fields.is_empty() {
        return Ok(());
    }

    writeln!(writer)?;
    writeln!(writer, "## EXIF Metadata")?;
    writeln!(writer)?;
    writeln!(writer, "| Tag | Value |")?;
    writeln!(writer, "|-----|-------|")?;
    for (tag, value) in &fields {
        writeln!(writer, "| {tag} | {} |", value.replace('|', "\\|"))?;
    }

    Ok(())
}

fn is_svg(input: &[u8]) -> bool {
    let header = if input.len() > 256 { &input[..256] } else { input };
    let text = String::from_utf8_lossy(header);
    text.contains("<svg") || text.starts_with("<?xml")
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;

    if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
