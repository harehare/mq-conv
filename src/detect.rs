use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Excel,
    Pdf,
    PowerPoint,
    Word,
    Image,
    Ipynb,
    Eml,
    Odt,
    Odp,
    Zip,
    Epub,
    Mobi,
    Audio,
    Sqlite,
    Tar,
    Video,
    Ocr,
    MarkdownDocx,
    MarkdownHtml,
    MarkdownText,
    MarkdownLatex,
    MarkdownRst,
    MarkdownAsciidoc,
    MarkdownOrg,
    MarkdownEpub,
    MarkdownJsonAst,
    Rst,
    Org,
    MediaWiki,
    Asciidoc,
}

impl Format {
    pub fn detect(filename: Option<&str>, bytes: &[u8]) -> Option<Self> {
        if let Some(name) = filename
            && let Some(fmt) = Self::from_extension(name)
        {
            return Some(fmt);
        }
        Self::from_magic_bytes(bytes)
    }

    fn from_extension(filename: &str) -> Option<Self> {
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())?;

        match ext.as_str() {
            "xlsx" | "xls" | "xlsb" | "ods" => Some(Self::Excel),
            "pdf" => Some(Self::Pdf),
            "pptx" => Some(Self::PowerPoint),
            "docx" => Some(Self::Word),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "tiff" | "tif" => {
                Some(Self::Image)
            }
            "ipynb" => Some(Self::Ipynb),
            "eml" => Some(Self::Eml),
            "odt" => Some(Self::Odt),
            "odp" => Some(Self::Odp),
            "zip" => Some(Self::Zip),
            "epub" => Some(Self::Epub),
            "mobi" | "azw" | "azw3" | "prc" => Some(Self::Mobi),
            "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "wma" => Some(Self::Audio),
            "sqlite" | "sqlite3" | "db" => Some(Self::Sqlite),
            "tar" => Some(Self::Tar),
            "tgz" => Some(Self::Tar),
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "m4v" | "wmv" | "flv" => Some(Self::Video),
            "md" | "markdown" => Some(Self::MarkdownDocx),
            "rst" => Some(Self::Rst),
            "org" => Some(Self::Org),
            "wiki" | "mediawiki" => Some(Self::MediaWiki),
            "adoc" | "asciidoc" => Some(Self::Asciidoc),
            _ => None,
        }
    }

    fn from_magic_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 4 {
            return None;
        }

        // PDF: %PDF
        if bytes.starts_with(b"%PDF") {
            return Some(Self::Pdf);
        }

        // MOBI / AZW: PalmDB header with the "BOOKMOBI" type and creator at offset 60
        if bytes.len() >= 68 && &bytes[60..68] == b"BOOKMOBI" {
            return Some(Self::Mobi);
        }

        // PNG: \x89PNG
        if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
            return Some(Self::Image);
        }

        // JPEG: \xFF\xD8\xFF
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(Self::Image);
        }

        // GIF: GIF87a or GIF89a
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            return Some(Self::Image);
        }

        // RIFF....WAVE (WAV)
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WAVE" {
            return Some(Self::Audio);
        }

        // FLAC
        if bytes.starts_with(b"fLaC") {
            return Some(Self::Audio);
        }

        // OGG
        if bytes.starts_with(b"OggS") {
            return Some(Self::Audio);
        }

        // MP3: ID3 tag or sync bytes
        if bytes.starts_with(b"ID3")
            || bytes.starts_with(&[0xFF, 0xFB])
            || bytes.starts_with(&[0xFF, 0xF3])
            || bytes.starts_with(&[0xFF, 0xF2])
        {
            return Some(Self::Audio);
        }

        // BMP
        if bytes.starts_with(b"BM") {
            return Some(Self::Image);
        }

        // TIFF
        if bytes.starts_with(&[0x49, 0x49, 0x2A, 0x00])
            || bytes.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
        {
            return Some(Self::Image);
        }

        // WEBP: RIFF....WEBP
        if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
            return Some(Self::Image);
        }

        // SQLite: "SQLite format 3\0"
        if bytes.len() >= 16 && bytes.starts_with(b"SQLite format 3\0") {
            return Some(Self::Sqlite);
        }

        // Gzip (tar.gz): \x1F\x8B
        if bytes.starts_with(&[0x1F, 0x8B]) {
            return Some(Self::Tar);
        }

        // ZIP-based formats: PK\x03\x04
        if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
            #[cfg(any(
                feature = "zip",
                feature = "word",
                feature = "powerpoint",
                feature = "excel",
                feature = "epub",
                feature = "opendocument"
            ))]
            return Self::detect_zip_content(bytes);
            #[cfg(not(any(
                feature = "zip",
                feature = "word",
                feature = "powerpoint",
                feature = "excel",
                feature = "epub",
                feature = "opendocument"
            )))]
            return Some(Self::Zip);
        }

        None
    }

    #[cfg(any(
        feature = "zip",
        feature = "word",
        feature = "powerpoint",
        feature = "excel",
        feature = "epub",
        feature = "opendocument"
    ))]
    fn detect_zip_content(bytes: &[u8]) -> Option<Self> {
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).ok()?;

        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).ok()?;
            let name = entry.name().ok()?.into_owned();

            if name.starts_with("word/") {
                return Some(Self::Word);
            }
            if name.starts_with("ppt/") {
                return Some(Self::PowerPoint);
            }
            if name.starts_with("xl/") {
                return Some(Self::Excel);
            }
            if name == "META-INF/container.xml" {
                return Some(Self::Epub);
            }
            // EPUB and OpenDocument both start with a `mimetype` entry.
            if name == "mimetype" {
                let mut mime = String::new();
                let _ = std::io::Read::read_to_string(&mut entry, &mut mime);
                return Some(match mime.trim() {
                    "application/vnd.oasis.opendocument.text" => Self::Odt,
                    "application/vnd.oasis.opendocument.presentation" => Self::Odp,
                    "application/vnd.oasis.opendocument.spreadsheet" => Self::Excel,
                    _ => Self::Epub,
                });
            }
        }

        Some(Self::Zip)
    }
}

impl Format {
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "excel" => Some(Self::Excel),
            "pdf" => Some(Self::Pdf),
            "powerpoint" => Some(Self::PowerPoint),
            "word" => Some(Self::Word),
            "image" => Some(Self::Image),
            "ipynb" => Some(Self::Ipynb),
            "eml" => Some(Self::Eml),
            "odt" => Some(Self::Odt),
            "odp" => Some(Self::Odp),
            "zip" => Some(Self::Zip),
            "epub" => Some(Self::Epub),
            "mobi" => Some(Self::Mobi),
            "audio" => Some(Self::Audio),
            "sqlite" => Some(Self::Sqlite),
            "tar" => Some(Self::Tar),
            "video" => Some(Self::Video),
            "ocr" => Some(Self::Ocr),
            "markdown-docx" => Some(Self::MarkdownDocx),
            "markdown-html" => Some(Self::MarkdownHtml),
            "markdown-text" => Some(Self::MarkdownText),
            "markdown-latex" => Some(Self::MarkdownLatex),
            "markdown-rst" => Some(Self::MarkdownRst),
            "markdown-asciidoc" => Some(Self::MarkdownAsciidoc),
            "markdown-org" => Some(Self::MarkdownOrg),
            "markdown-epub" => Some(Self::MarkdownEpub),
            "markdown-json-ast" => Some(Self::MarkdownJsonAst),
            "rst" => Some(Self::Rst),
            "org" => Some(Self::Org),
            "mediawiki" => Some(Self::MediaWiki),
            "asciidoc" => Some(Self::Asciidoc),
            _ => None,
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Excel => write!(f, "excel"),
            Self::Pdf => write!(f, "pdf"),
            Self::PowerPoint => write!(f, "powerpoint"),
            Self::Word => write!(f, "word"),
            Self::Image => write!(f, "image"),
            Self::Ipynb => write!(f, "ipynb"),
            Self::Eml => write!(f, "eml"),
            Self::Odt => write!(f, "odt"),
            Self::Odp => write!(f, "odp"),
            Self::Zip => write!(f, "zip"),
            Self::Epub => write!(f, "epub"),
            Self::Mobi => write!(f, "mobi"),
            Self::Audio => write!(f, "audio"),
            Self::Sqlite => write!(f, "sqlite"),
            Self::Tar => write!(f, "tar"),
            Self::Video => write!(f, "video"),
            Self::Ocr => write!(f, "ocr"),
            Self::MarkdownDocx => write!(f, "markdown-docx"),
            Self::MarkdownHtml => write!(f, "markdown-html"),
            Self::MarkdownText => write!(f, "markdown-text"),
            Self::MarkdownLatex => write!(f, "markdown-latex"),
            Self::MarkdownRst => write!(f, "markdown-rst"),
            Self::MarkdownAsciidoc => write!(f, "markdown-asciidoc"),
            Self::MarkdownOrg => write!(f, "markdown-org"),
            Self::MarkdownEpub => write!(f, "markdown-epub"),
            Self::MarkdownJsonAst => write!(f, "markdown-json-ast"),
            Self::Rst => write!(f, "rst"),
            Self::Org => write!(f, "org"),
            Self::MediaWiki => write!(f, "mediawiki"),
            Self::Asciidoc => write!(f, "asciidoc"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_new_formats_by_extension() {
        for (name, format) in [
            ("a.ipynb", Format::Ipynb),
            ("a.EML", Format::Eml),
            ("a.odt", Format::Odt),
            ("a.odp", Format::Odp),
        ] {
            assert_eq!(Format::detect(Some(name), b""), Some(format), "{name}");
            assert_eq!(Format::from_name(&format.to_string()), Some(format));
        }
    }

    #[cfg(feature = "zip")]
    #[test]
    fn zip_with_a_mimetype_entry_is_told_apart_by_its_content() {
        use std::io::Write;
        let zip_with_mimetype = |mime: &str| {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            zip.start_file("mimetype", opts).unwrap();
            zip.write_all(mime.as_bytes()).unwrap();
            zip.finish().unwrap().into_inner()
        };
        for (mime, format) in [
            ("application/vnd.oasis.opendocument.text", Format::Odt),
            (
                "application/vnd.oasis.opendocument.presentation",
                Format::Odp,
            ),
            (
                "application/vnd.oasis.opendocument.spreadsheet",
                Format::Excel,
            ),
            ("application/epub+zip", Format::Epub),
        ] {
            assert_eq!(
                Format::detect(None, &zip_with_mimetype(mime)),
                Some(format),
                "{mime}"
            );
        }
    }

    #[test]
    fn detects_mobi() {
        for name in ["a.mobi", "a.AZW3", "a.azw", "a.prc"] {
            assert_eq!(Format::detect(Some(name), b""), Some(Format::Mobi));
        }
        let mut bytes = vec![0u8; 80];
        bytes[60..68].copy_from_slice(b"BOOKMOBI");
        assert_eq!(Format::detect(None, &bytes), Some(Format::Mobi));
    }
}
