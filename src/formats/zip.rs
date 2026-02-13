use std::io::{Cursor, Write};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct ZipConverter;

impl Converter for ZipConverter {
    fn format_name(&self) -> &'static str {
        "zip"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let cursor = Cursor::new(input);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| Error::Conversion {
            format: "zip",
            message: e.to_string(),
        })?;

        let mut total_uncompressed: u64 = 0;
        let mut total_compressed: u64 = 0;
        let count = archive.len();

        writeln!(writer, "# Archive")?;
        writeln!(writer)?;
        writeln!(writer, "**Total entries**: {count}")?;
        writeln!(writer)?;

        writeln!(
            writer,
            "| # | Name | Size | Compressed | Method |"
        )?;
        writeln!(
            writer,
            "|---|------|------|------------|--------|"
        )?;

        for i in 0..count {
            let entry = archive.by_index(i).map_err(|e| Error::Conversion {
                format: "zip",
                message: e.to_string(),
            })?;

            let name = entry.name().to_string();
            let size = entry.size();
            let compressed = entry.compressed_size();
            let method = format!("{:?}", entry.compression());

            total_uncompressed += size;
            total_compressed += compressed;

            let (size_str, compressed_str) = if entry.is_dir() {
                ("-".to_string(), "-".to_string())
            } else {
                (format_size(size), format_size(compressed))
            };

            writeln!(
                writer,
                "| {idx} | {name} | {size_str} | {compressed_str} | {method} |",
                idx = i + 1,
            )?;
        }

        writeln!(writer)?;
        let ratio = if total_uncompressed > 0 {
            format!(
                "{:.1}%",
                (1.0 - total_compressed as f64 / total_uncompressed as f64) * 100.0
            )
        } else {
            "N/A".to_string()
        };
        writeln!(
            writer,
            "**Total size**: {} (compressed: {}, ratio: {ratio})",
            format_size(total_uncompressed),
            format_size(total_compressed),
        )?;

        Ok(())
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
