//! Shared helpers for writing embedded media (images) next to the Markdown
//! output and referencing them from it.

#[cfg(any(feature = "word", feature = "powerpoint"))]
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Writes media files into a directory, keeping file names unique.
pub struct MediaWriter {
    dir: PathBuf,
    used: HashSet<String>,
}

impl MediaWriter {
    pub fn new(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            used: HashSet::new(),
        })
    }

    /// Save `data` under a sanitised, unique version of `hint`'s file name and
    /// return the Markdown-ready path of the written file.
    pub fn save(&mut self, hint: &str, data: &[u8]) -> Option<String> {
        let base = hint.rsplit(['/', '\\']).next().unwrap_or(hint);
        let clean: String = base
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let clean = if clean.trim_matches('.').is_empty() {
            "image".to_string()
        } else {
            clean
        };
        let (stem, ext) = match clean.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (clean.clone(), String::new()),
        };
        let mut name = clean.clone();
        let mut n = 1;
        while !self.used.insert(name.clone()) {
            n += 1;
            name = format!("{stem}-{n}{ext}");
        }
        let path = self.dir.join(&name);
        std::fs::write(&path, data).ok()?;
        Some(md_path(&path))
    }
}

/// Path as written in Markdown: forward slashes, angle-bracketed when it
/// contains characters that would end the link destination.
pub fn md_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    if s.contains([' ', '(', ')']) {
        format!("<{s}>")
    } else {
        s
    }
}

pub fn is_image_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".tif", ".tiff", ".webp", ".svg",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
}

/// Resolve `target` relative to `base_dir` inside an archive, handling `..`.
pub fn resolve_archive_path(base_dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = base_dir.split('/').filter(|p| !p.is_empty()).collect();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

#[cfg(any(feature = "word", feature = "powerpoint"))]
pub fn parse_relationships(xml: &str) -> HashMap<String, String> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut rels = HashMap::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if e.name().as_ref().ends_with("Relationship") =>
            {
                let (mut id, mut target) = (None, None);
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        "Id" => id = Some(attr.value.to_string()),
                        "Target" => target = Some(attr.value.to_string()),
                        _ => {}
                    }
                }
                if let (Some(id), Some(target)) = (id, target) {
                    rels.insert(id, target);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    rels
}

/// Extract every image referenced by `rels` (relationship id → target) from a
/// zip archive into `dir`. Returns relationship id → Markdown path.
#[cfg(any(feature = "word", feature = "powerpoint"))]
pub fn extract_related_images(
    archive: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    base_dir: &str,
    rels: &HashMap<String, String>,
    dir: &Path,
) -> HashMap<String, String> {
    use std::io::Read;

    let mut out = HashMap::new();
    let Ok(mut writer) = MediaWriter::new(dir) else {
        return out;
    };
    let mut ids: Vec<&String> = rels.keys().collect();
    ids.sort();
    for id in ids {
        let target = &rels[id];
        if !is_image_name(target) || target.contains("://") {
            continue;
        }
        let full = resolve_archive_path(base_dir, target);
        let Ok(mut file) = archive.by_name(&full) else {
            continue;
        };
        let mut data = Vec::new();
        if file.read_to_end(&mut data).is_ok()
            && let Some(path) = writer.save(&full, &data)
        {
            out.insert(id.clone(), path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_sanitises_and_deduplicates_names() {
        let dir = std::env::temp_dir().join(format!("mq-conv-media-{}", std::process::id()));
        let mut w = MediaWriter::new(&dir).unwrap();
        let a = w.save("word/media/my image (1).png", b"a").unwrap();
        let b = w.save("ppt/media/my image (1).png", b"b").unwrap();
        assert_ne!(a, b);
        assert!(a.ends_with("my_image__1_.png"), "{a}");
        assert!(b.ends_with("my_image__1_-2.png"), "{b}");
        assert_eq!(std::fs::read(dir.join("my_image__1_.png")).unwrap(), b"a");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn md_path_quotes_paths_with_spaces() {
        assert_eq!(md_path(Path::new("a/b.png")), "a/b.png");
        assert_eq!(md_path(Path::new("my dir/b.png")), "<my dir/b.png>");
    }

    #[test]
    fn resolves_archive_paths() {
        assert_eq!(
            resolve_archive_path("word/", "media/a.png"),
            "word/media/a.png"
        );
        assert_eq!(
            resolve_archive_path("OEBPS/text/", "../img/a.png"),
            "OEBPS/img/a.png"
        );
        assert_eq!(resolve_archive_path("word/", "/x/a.png"), "x/a.png");
    }

    #[test]
    fn recognises_image_names() {
        assert!(is_image_name("a.PNG"));
        assert!(!is_image_name("a.xml"));
    }
}
