use std::io::{Cursor, Write};

use crate::converter::Converter;
use crate::document::{Block, Document};
use crate::error::{Error, Result};
use crate::formats::media::format_size;

pub struct ImageConverter;

impl Converter for ImageConverter {
    fn format_name(&self) -> &'static str {
        "image"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let mut doc = Document::new();
        doc.push(Block::heading(1, "Image"));

        if is_svg(input) {
            doc.push(Block::key_value_table(
                ["Property", "Value"],
                &[
                    ("Format", "SVG".to_string()),
                    ("Size", format_size(input.len() as u64)),
                ],
            ));
            return doc.write_markdown(writer);
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

        let mut rows = Vec::new();
        if let Some(fmt) = format {
            rows.push(("Format", format!("{fmt:?}")));
        }
        rows.push(("Size", format_size(input.len() as u64)));
        rows.push(("Dimensions", format!("{}x{}", img.width(), img.height())));
        rows.push(("Color Type", format!("{:?}", img.color())));
        doc.push(Block::key_value_table(["Property", "Value"], &rows));

        if let Some(exif) = exif_blocks(input) {
            doc.extend(exif);
        }

        doc.write_markdown(writer)
    }
}

fn exif_blocks(input: &[u8]) -> Option<Vec<Block>> {
    let mut cursor = Cursor::new(input);
    let exif_data: exif::Exif = exif::Reader::new().read_from_container(&mut cursor).ok()?;

    let fields: Vec<(String, String)> = exif_data
        .fields()
        .filter_map(|f| {
            let value = f.display_value().with_unit(&exif_data).to_string();
            if value.is_empty() || value == "unknown" {
                return None;
            }
            Some((f.tag.to_string(), value))
        })
        .collect();

    if fields.is_empty() {
        return None;
    }
    Some(vec![
        Block::heading(2, "EXIF Metadata"),
        Block::key_value_table(["Tag", "Value"], &fields),
    ])
}

fn is_svg(input: &[u8]) -> bool {
    let header = if input.len() > 256 {
        &input[..256]
    } else {
        input
    };
    let text = String::from_utf8_lossy(header);
    text.contains("<svg") || text.starts_with("<?xml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(input: &[u8]) -> String {
        let mut out = Vec::new();
        ImageConverter.convert(input, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn describes_a_png() {
        let mut png = Cursor::new(Vec::new());
        ::image::RgbImage::new(2, 3)
            .write_to(&mut png, ::image::ImageFormat::Png)
            .unwrap();
        let out = convert(&png.into_inner());
        assert!(out.starts_with("# Image\n\n| Property"), "{out}");
        assert!(out.contains("| Format     | Png"), "{out}");
        assert!(out.contains("| Dimensions | 2x3"), "{out}");
        assert!(out.contains("| Color Type | Rgb8"), "{out}");
    }

    #[test]
    fn describes_an_svg_without_decoding_it() {
        let out = convert(b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>");
        assert!(out.contains("| Format   | SVG"), "{out}");
    }
}
