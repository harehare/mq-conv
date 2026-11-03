use std::io::{Cursor, Read, Write};

use crate::converter::Converter;
use crate::document::{Block, Document};
use crate::error::{Error, Result};
use crate::formats::media::format_size;

pub struct TarConverter;

impl Converter for TarConverter {
    fn format_name(&self) -> &'static str {
        "tar"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        // Try gzip first, then plain tar
        if is_gzip(input) {
            let decoder = flate2::read::GzDecoder::new(Cursor::new(input));
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

    let mut doc = Document::new();
    doc.push(Block::heading(1, "Archive"));
    doc.push(Block::field("Total entries", items.len()));

    let mut rows = vec![["#", "Name", "Size", "Type"].map(String::from).to_vec()];
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
        rows.push(vec![
            (idx + 1).to_string(),
            name.clone(),
            size_str,
            type_str.to_string(),
        ]);
    }
    doc.push(Block::text_table(&rows));
    doc.push(Block::field("Total size", format_size(total_size)));
    doc.write_markdown(writer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tar_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = ::tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut header = ::tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, name, *data).unwrap();
        }
        builder.into_inner().unwrap()
    }

    #[test]
    fn lists_entries_and_totals() {
        let bytes = tar_with(&[("a.txt", b"hello"), ("dir/b.txt", &[0u8; 2048])]);
        let mut out = Vec::new();
        TarConverter.convert(&bytes, &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.starts_with("# Archive\n\n**Total entries**: 2\n\n"),
            "{out}"
        );
        assert!(out.contains("| 1 | a.txt "), "{out}");
        assert!(out.contains("2.0 KB"), "{out}");
        assert!(out.ends_with("**Total size**: 2.0 KB\n"), "{out}");
    }
}
