use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Excel,
    Pdf,
    PowerPoint,
    Word,
    Image,
    Zip,
    Epub,
    Audio,
    Csv,
    Html,
    Json,
    Yaml,
    Toml,
    Xml,
    Sqlite,
    Tar,
    Video,
}

impl Format {
    pub fn detect(filename: Option<&str>, bytes: &[u8]) -> Option<Self> {
        if let Some(name) = filename
            && let Some(fmt) = Self::from_extension(name) {
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
            "zip" => Some(Self::Zip),
            "epub" => Some(Self::Epub),
            "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "wma" => Some(Self::Audio),
            "csv" | "tsv" => Some(Self::Csv),
            "html" | "htm" => Some(Self::Html),
            "json" => Some(Self::Json),
            "yaml" | "yml" => Some(Self::Yaml),
            "toml" => Some(Self::Toml),
            "xml" => Some(Self::Xml),
            "sqlite" | "sqlite3" | "db" => Some(Self::Sqlite),
            "tar" => Some(Self::Tar),
            "tgz" => Some(Self::Tar),
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "m4v" | "wmv" | "flv" => {
                Some(Self::Video)
            }
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
                feature = "epub"
            ))]
            return Self::detect_zip_content(bytes);
            #[cfg(not(any(
                feature = "zip",
                feature = "word",
                feature = "powerpoint",
                feature = "excel",
                feature = "epub"
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
        feature = "epub"
    ))]
    fn detect_zip_content(bytes: &[u8]) -> Option<Self> {
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor).ok()?;

        for i in 0..archive.len() {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();

            if name.starts_with("word/") {
                return Some(Self::Word);
            }
            if name.starts_with("ppt/") {
                return Some(Self::PowerPoint);
            }
            if name.starts_with("xl/") {
                return Some(Self::Excel);
            }
            if name == "mimetype" || name == "META-INF/container.xml" {
                return Some(Self::Epub);
            }
        }

        Some(Self::Zip)
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
            Self::Zip => write!(f, "zip"),
            Self::Epub => write!(f, "epub"),
            Self::Audio => write!(f, "audio"),
            Self::Csv => write!(f, "csv"),
            Self::Html => write!(f, "html"),
            Self::Json => write!(f, "json"),
            Self::Yaml => write!(f, "yaml"),
            Self::Toml => write!(f, "toml"),
            Self::Xml => write!(f, "xml"),
            Self::Sqlite => write!(f, "sqlite"),
            Self::Tar => write!(f, "tar"),
            Self::Video => write!(f, "video"),
        }
    }
}
