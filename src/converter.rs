use crate::error::{Error, Result};
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;

/// Options shared by converters that support them; converters ignore the
/// fields they do not use.
#[derive(Debug, Clone, Default)]
pub struct ConvertOptions {
    /// Write embedded images into this directory and reference them from the
    /// Markdown output.
    pub image_dir: Option<PathBuf>,
    /// Directory prefix used in Markdown image links. Defaults to
    /// `image_dir`; set it when the Markdown is saved somewhere other than
    /// the working directory so links stay valid relative to that file.
    pub image_link_dir: Option<PathBuf>,
    /// Tesseract language used for OCR fallbacks (default "eng").
    pub ocr_lang: Option<String>,
    /// Convert only these pages (PDF pages, PowerPoint slides, Excel sheets).
    /// Numbers in the output stay those of the source document.
    pub pages: Option<PageRanges>,
}

impl ConvertOptions {
    /// Whether the 1-based `page` is wanted; true when no selection was given.
    pub fn is_selected(&self, page: u32) -> bool {
        self.pages.as_ref().is_none_or(|p| p.contains(page))
    }

    /// Fail when `--pages` is set but matches none of `total` pages. `unit`
    /// names what the format calls a page ("page", "slide", "sheet").
    pub fn check_pages(&self, format: &'static str, unit: &str, total: usize) -> Result<()> {
        let any = (1..=total).any(|n| self.is_selected(n as u32));
        if any {
            return Ok(());
        }
        Err(Error::Conversion {
            format,
            message: format!("no {unit} matches --pages (the document has {total} {unit}s)"),
        })
    }
}

/// A set of 1-based page numbers, written like `3`, `2-5`, `7-` or `1,4-6`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRanges(Vec<(u32, u32)>);

impl PageRanges {
    pub fn contains(&self, page: u32) -> bool {
        self.0.iter().any(|&(lo, hi)| (lo..=hi).contains(&page))
    }
}

impl FromStr for PageRanges {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let page = |t: &str| {
            t.trim()
                .parse::<u32>()
                .ok()
                .filter(|&n| n > 0)
                .ok_or_else(|| format!("invalid page number '{}'", t.trim()))
        };
        let mut ranges = Vec::new();
        for part in s.split(',') {
            let part = part.trim();
            let range = match part.split_once('-') {
                None => {
                    let n = page(part)?;
                    (n, n)
                }
                Some((lo, hi)) => {
                    if lo.trim().is_empty() && hi.trim().is_empty() {
                        return Err("page range '-' needs a start or an end".to_string());
                    }
                    let lo = if lo.trim().is_empty() { 1 } else { page(lo)? };
                    let hi = if hi.trim().is_empty() {
                        u32::MAX
                    } else {
                        page(hi)?
                    };
                    if lo > hi {
                        return Err(format!("page range '{part}' is backwards"));
                    }
                    (lo, hi)
                }
            };
            ranges.push(range);
        }
        Ok(Self(ranges))
    }
}

pub trait Converter {
    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()>;

    /// Like [`Converter::convert`] but honouring [`ConvertOptions`].
    fn convert_with(
        &self,
        input: &[u8],
        writer: &mut dyn Write,
        _options: &ConvertOptions,
    ) -> Result<()> {
        self.convert(input, writer)
    }

    fn format_name(&self) -> &'static str;
    fn output_extension(&self) -> &'static str {
        "md"
    }

    /// Whether the output is UTF-8 text. Binary outputs (DOCX, EPUB) must not
    /// go through text-oriented post-processing such as token budgeting.
    fn is_text_output(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(s: &str) -> PageRanges {
        s.parse().unwrap()
    }

    #[test]
    fn parses_pages_and_ranges() {
        let p = pages("1,3-4, 7-");
        let hits: Vec<u32> = (1..=9).filter(|&n| p.contains(n)).collect();
        assert_eq!(hits, vec![1, 3, 4, 7, 8, 9]);
        assert!(pages("-2").contains(1) && pages("-2").contains(2) && !pages("-2").contains(3));
        assert!(pages("5").contains(5) && !pages("5").contains(4));
    }

    #[test]
    fn rejects_bad_page_specs() {
        for bad in ["", "0", "a", "3-1", "1,,2", "1-2-3", "-"] {
            assert!(bad.parse::<PageRanges>().is_err(), "{bad:?} was accepted");
        }
    }
}
