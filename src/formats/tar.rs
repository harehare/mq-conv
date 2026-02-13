use std::io::{Cursor, Read, Write};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct TarConverter;

impl Converter for TarConverter {
    fn format_name(&self) -> &'static str {
        "tar"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        // Try gzip first, then plain tar
        if is_gzip(input) {
            let decoder =
                flate2::read::GzDecoder::new(Cursor::new(input));
            convert_tar(decoder, writer)
        } else {
            convert_tar(Cursor::new(input), writer)
        }
    }
}

fn is_gzip(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1F && bytes[1] == 0x8B
}

fn convert_tar<R: Read>(reader: R, writer: &mut dyn Write) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries().map_err(|e| Error::Conversion {
        format: "tar",
        message: e.to_string(),
    })?;

    let mut items: Vec<(String, u64, char)> = Vec::new();
    let mut total_size: u64 = 0;

    for entry in entries {
        let entry = entry.map_err(|e| Error::Conversion {
            format: "tar",
            message: e.to_string(),
        })?;

        let path = entry
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "???".to_string());

        let size = entry.size();
        let kind = match entry.header().entry_type() {
            tar::EntryType::Regular => 'f',
            tar::EntryType::Directory => 'd',
            tar::EntryType::Symlink => 'l',
            tar::EntryType::Link => 'h',
            _ => '?',
        };

        total_size += size;
        items.push((path, size, kind));
    }

    writeln!(writer, "# Archive")?;
    writeln!(writer)?;
    writeln!(writer, "**Total entries**: {}", items.len())?;
    writeln!(writer)?;

    writeln!(writer, "| # | Name | Size | Type |")?;
    writeln!(writer, "|---|------|------|------|")?;

    for (idx, (name, size, kind)) in items.iter().enumerate() {
        let type_str = match kind {
            'd' => "dir",
            'f' => "file",
            'l' => "symlink",
            'h' => "hardlink",
            _ => "other",
        };
        let size_str = if *kind == 'd' {
            "-".to_string()
        } else {
            format_size(*size)
        };
        writeln!(
            writer,
            "| {} | {name} | {size_str} | {type_str} |",
            idx + 1,
        )?;
    }

    writeln!(writer)?;
    writeln!(writer, "**Total size**: {}", format_size(total_size))?;

    Ok(())
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
