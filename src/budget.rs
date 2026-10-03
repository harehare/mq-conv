//! Token budgeting for large documents: split converted Markdown at block
//! boundaries and resume from a cursor, so an LLM client can page through a
//! document that does not fit its context window.
//!
//! A cursor is a byte offset into the converted Markdown. Conversion is
//! deterministic, so re-running the same command with `--cursor` continues
//! exactly where the previous chunk ended.

/// Rough token estimate: ~4 ASCII characters per token, about one token per
/// CJK character and half a token for other non-ASCII characters.
pub fn estimate_tokens(text: &str) -> usize {
    let mut quarters = 0usize; // in 1/4 token units
    for c in text.chars() {
        quarters += match c as u32 {
            0..=0x7F => 1,
            0x2E80..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
            | 0x20000..=0x2FFFF => 4,
            _ => 2,
        };
    }
    quarters.div_ceil(4)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub text: &'a str,
    /// Estimated tokens in `text`.
    pub tokens: usize,
    /// Offset to pass as `--cursor` for the next chunk, if any text remains.
    pub next_cursor: Option<usize>,
    /// Estimated tokens in the whole document.
    pub total_tokens: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CursorError {
    OutOfRange { cursor: usize, len: usize },
    NotCharBoundary(usize),
}

impl std::fmt::Display for CursorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CursorError::OutOfRange { cursor, len } => {
                write!(
                    f,
                    "cursor {cursor} is past the end of the document ({len} bytes)"
                )
            }
            CursorError::NotCharBoundary(c) => {
                write!(f, "cursor {c} does not point at a character boundary")
            }
        }
    }
}

impl std::error::Error for CursorError {}

/// Split `text` into blocks separated by blank lines (blank lines inside a
/// fenced code block do not split). Each block spans from its first line up
/// to, but excluding, the next block's first line, so concatenating all
/// blocks reproduces `text`.
fn block_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut pos = 0usize;
    let mut in_fence = false;
    let mut seen_blank = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let is_blank = trimmed.is_empty();
        if !is_blank && seen_blank && !in_fence {
            ranges.push((start, pos));
            start = pos;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        seen_blank = is_blank && !in_fence;
        pos += line.len();
    }
    if pos > start {
        ranges.push((start, pos));
    }
    ranges
}

/// Largest prefix of `text` (cut at line, then char boundaries) whose
/// estimated size is within `max_tokens`; at least one character.
fn split_oversized(text: &str, max_tokens: usize) -> usize {
    let mut end = 0usize;
    for line in text.split_inclusive('\n') {
        if estimate_tokens(&text[..end + line.len()]) > max_tokens {
            break;
        }
        end += line.len();
    }
    if end > 0 {
        return end;
    }
    // A single very long line: cut by characters.
    let mut last = 0usize;
    for (i, c) in text.char_indices() {
        let next = i + c.len_utf8();
        if last > 0 && estimate_tokens(&text[..next]) > max_tokens {
            break;
        }
        last = next;
    }
    last.max(text.chars().next().map_or(0, char::len_utf8))
}

/// Take the next chunk starting at byte offset `cursor`.
///
/// With `max_tokens == None` the whole remainder is returned.
pub fn take_chunk(
    text: &str,
    cursor: usize,
    max_tokens: Option<usize>,
) -> Result<Chunk<'_>, CursorError> {
    if cursor > text.len() {
        return Err(CursorError::OutOfRange {
            cursor,
            len: text.len(),
        });
    }
    if !text.is_char_boundary(cursor) {
        return Err(CursorError::NotCharBoundary(cursor));
    }
    let total_tokens = estimate_tokens(text);
    let rest = &text[cursor..];
    let Some(max) = max_tokens.map(|m| m.max(1)) else {
        return Ok(Chunk {
            text: rest,
            tokens: estimate_tokens(rest),
            next_cursor: None,
            total_tokens,
        });
    };

    let mut end = 0usize;
    for (s, e) in block_ranges(rest) {
        if estimate_tokens(&rest[..e]) <= max {
            end = e;
            continue;
        }
        if end == 0 {
            // The first block alone is over budget: split it.
            end = s + split_oversized(&rest[s..e], max);
        }
        break;
    }

    let chunk = &rest[..end];
    let next = cursor + end;
    Ok(Chunk {
        text: chunk,
        tokens: estimate_tokens(chunk),
        next_cursor: (next < text.len() && !text[next..].trim().is_empty()).then_some(next),
        total_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_tokens_for_ascii_and_cjk() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("日本語"), 3);
    }

    #[test]
    fn no_budget_returns_the_remainder() {
        let c = take_chunk("hello\n\nworld\n", 0, None).unwrap();
        assert_eq!(c.text, "hello\n\nworld\n");
        assert_eq!(c.next_cursor, None);
    }

    #[test]
    fn chunks_at_block_boundaries_and_resumes() {
        let doc = "aaaa aaaa aaaa\n\nbbbb bbbb bbbb\n\ncccc cccc cccc\n";
        let first = take_chunk(doc, 0, Some(5)).unwrap();
        assert_eq!(first.text, "aaaa aaaa aaaa\n\n");
        let second = take_chunk(doc, first.next_cursor.unwrap(), Some(5)).unwrap();
        assert_eq!(second.text, "bbbb bbbb bbbb\n\n");
        let third = take_chunk(doc, second.next_cursor.unwrap(), Some(5)).unwrap();
        assert_eq!(third.text, "cccc cccc cccc\n");
        assert_eq!(third.next_cursor, None);
    }

    #[test]
    fn chunks_reassemble_to_the_original() {
        let doc = "# T\n\npara one is here\n\n```\ncode\n\nmore code\n```\n\nlast paragraph\n";
        let mut cursor = 0;
        let mut out = String::new();
        loop {
            let c = take_chunk(doc, cursor, Some(6)).unwrap();
            out.push_str(c.text);
            match c.next_cursor {
                Some(n) => cursor = n,
                None => break,
            }
        }
        assert_eq!(out, doc);
    }

    #[test]
    fn code_fences_are_not_split_at_blank_lines() {
        let doc = "intro\n\n```\na\n\nb\n```\n\nend\n";
        let blocks = block_ranges(doc);
        let texts: Vec<&str> = blocks.iter().map(|&(s, e)| &doc[s..e]).collect();
        assert_eq!(texts, vec!["intro\n\n", "```\na\n\nb\n```\n\n", "end\n"]);
    }

    #[test]
    fn oversized_block_is_split_by_lines() {
        let doc = "l1 l1 l1 l1\nl2 l2 l2 l2\nl3 l3 l3 l3\n";
        let c = take_chunk(doc, 0, Some(5)).unwrap();
        assert_eq!(c.text, "l1 l1 l1 l1\n");
        assert!(c.next_cursor.is_some());
    }

    #[test]
    fn oversized_single_line_is_split_by_chars_on_boundaries() {
        let doc = "日本語のとても長い一行のテキスト";
        let c = take_chunk(doc, 0, Some(4)).unwrap();
        assert_eq!(c.text.chars().count(), 4);
        assert_eq!(c.next_cursor, Some(c.text.len()));
    }

    #[test]
    fn bad_cursors_are_rejected() {
        assert_eq!(
            take_chunk("abc", 9, None),
            Err(CursorError::OutOfRange { cursor: 9, len: 3 })
        );
        assert_eq!(
            take_chunk("日", 1, None),
            Err(CursorError::NotCharBoundary(1))
        );
    }
}
