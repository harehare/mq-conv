//! Shared helpers for writing embedded media (images) next to the Markdown
//! output and referencing them from it.

#[cfg(any(feature = "word", feature = "powerpoint"))]
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::converter::ConvertOptions;

/// Writes media files into a directory, keeping file names unique.
///
/// One writer must serve a whole document: uniqueness is tracked per writer,
/// and the same source `hint` is written only once.
pub struct MediaWriter {
    dir: PathBuf,
    /// Directory prefix of the Markdown links (may differ from `dir`).
    link_dir: PathBuf,
    used: HashSet<String>,
    saved: std::collections::HashMap<String, String>,
}

impl MediaWriter {
    pub fn new(dir: &Path, link_dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        Ok(Self {
            dir: dir.to_path_buf(),
            link_dir: link_dir.to_path_buf(),
            used: HashSet::new(),
            saved: std::collections::HashMap::new(),
        })
    }

    /// A writer for `options.image_dir`, if image extraction was requested
    /// and the directory could be created.
    pub fn from_options(options: &ConvertOptions) -> Option<Self> {
        let dir = options.image_dir.as_deref()?;
        let link_dir = options.image_link_dir.as_deref().unwrap_or(dir);
        Self::new(dir, link_dir).ok()
    }

    /// Markdown path of `hint` if it was already saved by this writer.
    pub fn saved_path(&self, hint: &str) -> Option<&str> {
        self.saved.get(hint).map(String::as_str)
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
        std::fs::write(self.dir.join(&name), data).ok()?;
        let link = md_path(&self.link_dir.join(&name));
        self.saved.insert(hint.to_string(), link.clone());
        Some(link)
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

/// `target` as a path relative to the directory `from`, so a file saved in
/// `from` can link to it. Falls back to the absolute target when the two share
/// no root (e.g. different drives).
pub fn relative_path(from: &Path, target: &Path) -> PathBuf {
    use std::path::Component;

    fn normalized(p: &Path) -> Vec<Component<'_>> {
        let mut out: Vec<Component> = Vec::new();
        for c in p.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir if matches!(out.last(), Some(Component::Normal(_))) => {
                    out.pop();
                }
                c => out.push(c),
            }
        }
        out
    }

    let (Ok(from_abs), Ok(target_abs)) = (std::path::absolute(from), std::path::absolute(target))
    else {
        return target.to_path_buf();
    };
    let (from_parts, target_parts) = (normalized(&from_abs), normalized(&target_abs));
    let common = from_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return target_abs;
    }
    let mut rel = PathBuf::new();
    for _ in common..from_parts.len() {
        rel.push("..");
    }
    for c in &target_parts[common..] {
        rel.push(c);
    }
    rel
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
    writer: &mut MediaWriter,
) -> HashMap<String, String> {
    use std::io::Read;

    let mut out = HashMap::new();
    let mut ids: Vec<&String> = rels.keys().collect();
    ids.sort();
    for id in ids {
        let target = &rels[id];
        if !is_image_name(target) || target.contains("://") {
            continue;
        }
        let full = resolve_archive_path(base_dir, target);
        if let Some(path) = writer.saved_path(&full) {
            out.insert(id.clone(), path.to_string());
            continue;
        }
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
        let mut w = MediaWriter::new(&dir, &dir).unwrap();
        let a = w.save("word/media/my image (1).png", b"a").unwrap();
        let b = w.save("ppt/media/my image (1).png", b"b").unwrap();
        assert_ne!(a, b);
        assert!(a.ends_with("my_image__1_.png"), "{a}");
        assert!(b.ends_with("my_image__1_-2.png"), "{b}");
        assert_eq!(std::fs::read(dir.join("my_image__1_.png")).unwrap(), b"a");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn relative_path_links_from_the_markdown_directory() {
        let rel = |from: &str, to: &str| relative_path(Path::new(from), Path::new(to));
        assert_eq!(rel("out", "img"), Path::new("../img"));
        assert_eq!(rel("out", "out/img"), Path::new("img"));
        assert_eq!(rel("out/docs", "img/a"), Path::new("../../img/a"));
        assert_eq!(rel("out/../out", "./img"), Path::new("../img"));
        assert_eq!(rel("out", "out"), Path::new(""));
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
