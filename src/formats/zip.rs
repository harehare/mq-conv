use std::io::{Cursor, Write};

use crate::converter::Converter;
use crate::document::{Block, Document};
use crate::error::{Error, Result};
use crate::formats::media::format_size;

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

        let mut doc = Document::new();
        doc.push(Block::heading(1, "Archive"));
        doc.push(Block::field("Total entries", count));

        let mut rows = vec![
            ["#", "Name", "Size", "Compressed", "Method"]
                .map(String::from)
                .to_vec(),
        ];
        for i in 0..count {
            let entry = archive.by_index(i).map_err(|e| Error::Conversion {
                format: "zip",
                message: e.to_string(),
            })?;

            let name = entry
                .name()
                .map_err(|e| Error::Conversion {
                    format: "zip",
                    message: e.to_string(),
                })?
                .into_owned();
            let size = entry.size();
            let compressed = entry.compressed_size();
            total_uncompressed += size;
            total_compressed += compressed;

            let (size_str, compressed_str) = if entry.is_dir() {
                ("-".to_string(), "-".to_string())
            } else {
                (format_size(size), format_size(compressed))
            };
            rows.push(vec![
                (i + 1).to_string(),
                name,
                size_str,
                compressed_str,
                format!("{:?}", entry.compression()),
            ]);
        }
        doc.push(Block::text_table(&rows));

        let ratio = if total_uncompressed > 0 {
            format!(
                "{:.1}%",
                (1.0 - total_compressed as f64 / total_uncompressed as f64) * 100.0
            )
        } else {
            "N/A".to_string()
        };
        doc.push(Block::field(
            "Total size",
            format!(
                "{} (compressed: {}, ratio: {ratio})",
                format_size(total_uncompressed),
                format_size(total_compressed)
            ),
        ));
        doc.write_markdown(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_entries_and_the_compression_ratio() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut zip = ::zip::ZipWriter::new(&mut buf);
            let options = ::zip::write::SimpleFileOptions::default()
                .compression_method(::zip::CompressionMethod::Deflated);
            zip.start_file("a.txt", options).unwrap();
            zip.write_all(&[b'a'; 4096]).unwrap();
            zip.finish().unwrap();
        }
        let mut out = Vec::new();
        ZipConverter.convert(&buf.into_inner(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.starts_with("# Archive\n\n**Total entries**: 1\n\n"),
            "{out}"
        );
        assert!(out.contains("| 1 | a.txt "), "{out}");
        assert!(out.contains("4.0 KB"), "{out}");
        assert!(
            out.contains("**Total size**: 4.0 KB (compressed: "),
            "{out}"
        );
    }
}
