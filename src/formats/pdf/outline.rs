//! The PDF outline (bookmarks) as a source of heading levels.
//!
//! Layout analysis guesses headings from font sizes. When the author wrote
//! bookmarks they are a better guide: each names a heading and its depth. An
//! entry is matched to the block on its page that carries the same text and
//! that block becomes a heading of the bookmark's level.

use lopdf::Document;

use crate::document::{Block, Inline, plain_text};

/// One bookmark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Depth in the outline, 1 for the top level.
    pub level: u8,
    pub title: String,
    /// 1-based page number the bookmark points to.
    pub page: u32,
}

/// The bookmarks of `doc`, in outline order; empty when there are none.
pub fn read(doc: &Document) -> Vec<Entry> {
    let Ok(toc) = doc.get_toc() else {
        return Vec::new();
    };
    toc.toc
        .into_iter()
        .filter_map(|t| {
            let title = t.title.trim().to_string();
            (!title.is_empty()).then_some(Entry {
                level: t.level.clamp(1, 6) as u8,
                title,
                page: u32::try_from(t.page).ok()?,
            })
        })
        .collect()
}

/// Turns the blocks of `page` that match one of its bookmarks into headings.
pub fn apply(blocks: &mut [Block], entries: &[Entry], page: u32) {
    let mut from = 0;
    for entry in entries.iter().filter(|e| e.page == page) {
        let wanted = key(&entry.title);
        if wanted.is_empty() {
            continue;
        }
        let found = blocks
            .iter()
            .enumerate()
            .skip(from)
            .find(|(_, block)| matches_title(block, &wanted))
            .map(|(i, _)| i);
        let Some(i) = found else { continue };
        let content = match &blocks[i] {
            Block::Heading { content, .. } | Block::Paragraph(content) => content.clone(),
            _ => continue,
        };
        blocks[i] = Block::Heading {
            level: entry.level,
            content,
        };
        from = i + 1;
    }
}

/// Whether `block` is a line of text that is the bookmark `wanted`. A title
/// that wraps over two lines may come out as just its first part, so a block
/// that is a prefix of the title counts too; a long paragraph that merely
/// begins with the title does not.
fn matches_title(block: &Block, wanted: &str) -> bool {
    let (Block::Heading { content, .. } | Block::Paragraph(content)) = block else {
        return false;
    };
    if content.iter().any(|i| matches!(i, Inline::Image { .. })) {
        return false;
    }
    let text = key(&plain_text(content));
    if text.is_empty() {
        return false;
    }
    text == wanted || (text.chars().count() >= 6 && wanted.starts_with(&text))
}

/// Text reduced to what identifies a heading: lower-case letters and digits,
/// without the section number a bookmark may or may not repeat.
fn key(text: &str) -> String {
    let words: String = text
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    let words = words.trim_start();
    let without_number = words.trim_start_matches(|c: char| c.is_ascii_digit());
    // "1 Intro" and "Intro" agree, but "2020" or "3D models" keep their digits.
    let stripped = if without_number.starts_with(char::is_whitespace) {
        without_number
    } else {
        words
    };
    stripped.chars().filter(|c| !c.is_whitespace()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn para(text: &str) -> Block {
        Block::paragraph(text)
    }

    fn entry(level: u8, title: &str, page: u32) -> Entry {
        Entry {
            level,
            title: title.to_string(),
            page,
        }
    }

    fn heading_text(block: &Block) -> Option<(u8, String)> {
        match block {
            Block::Heading { level, content } => Some((*level, plain_text(content))),
            _ => None,
        }
    }

    #[test]
    fn a_matching_paragraph_becomes_a_heading_of_the_outline_level() {
        let mut blocks = vec![para("Intro text"), para("Methods"), para("Body")];
        apply(&mut blocks, &[entry(2, "Methods", 3)], 3);
        assert_eq!(heading_text(&blocks[1]), Some((2, "Methods".into())));
        assert!(heading_text(&blocks[0]).is_none() && heading_text(&blocks[2]).is_none());
    }

    #[test]
    fn a_detected_heading_takes_the_outline_level() {
        let mut blocks = vec![Block::heading(1, "Results")];
        apply(&mut blocks, &[entry(3, "Results", 1)], 1);
        assert_eq!(heading_text(&blocks[0]), Some((3, "Results".into())));
    }

    #[test]
    fn section_numbers_do_not_prevent_a_match() {
        let mut blocks = vec![para("1.2 Background")];
        apply(&mut blocks, &[entry(2, "Background", 1)], 1);
        assert!(heading_text(&blocks[0]).is_some());
        let mut blocks = vec![para("Background")];
        apply(&mut blocks, &[entry(2, "1.2 Background", 1)], 1);
        assert!(heading_text(&blocks[0]).is_some());
    }

    #[test]
    fn other_pages_and_long_paragraphs_are_left_alone() {
        let mut blocks = vec![para("Overview of everything we did in this long paragraph")];
        apply(&mut blocks, &[entry(1, "Overview", 1)], 1);
        assert!(heading_text(&blocks[0]).is_none());
        let mut blocks = vec![para("Overview")];
        apply(&mut blocks, &[entry(1, "Overview", 2)], 1);
        assert!(heading_text(&blocks[0]).is_none());
    }

    #[test]
    fn a_wrapped_title_matches_its_first_line() {
        let mut blocks = vec![para("A very long chapter title that")];
        apply(
            &mut blocks,
            &[entry(1, "A very long chapter title that wraps", 1)],
            1,
        );
        assert!(heading_text(&blocks[0]).is_some());
    }

    #[test]
    fn entries_match_in_order_and_each_block_once() {
        let mut blocks = vec![para("Part"), para("text"), para("Part")];
        apply(&mut blocks, &[entry(1, "Part", 1), entry(2, "Part", 1)], 1);
        assert_eq!(heading_text(&blocks[0]), Some((1, "Part".into())));
        assert_eq!(heading_text(&blocks[2]), Some((2, "Part".into())));
    }

    #[test]
    fn non_text_blocks_are_never_matched() {
        let mut blocks = vec![Block::Paragraph(vec![Inline::Image {
            url: "x.png".into(),
            alt: "Chart".into(),
        }])];
        apply(&mut blocks, &[entry(1, "Chart", 1)], 1);
        assert!(heading_text(&blocks[0]).is_none());
    }
}
