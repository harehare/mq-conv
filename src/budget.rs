//! Token budgeting for large documents: split converted Markdown at block
//! boundaries and resume from a cursor, so an LLM client can page through a
//! document that does not fit its context window.
//!
//! A cursor is a byte offset into the converted Markdown. Conversion is
//! deterministic, so re-running the same command with `--cursor` continues
//! exactly where the previous chunk ended.
//!
//! Blocks are cut at blank lines. A fenced code block or table larger than
//! the budget is cut between lines instead, and each piece is made valid on
//! its own: the fence is closed and re-opened, and the table header is
//! repeated. Table rows are never cut.

use std::borrow::Cow;

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
    /// The slice of the document, plus any fence or table header added so the
    /// chunk is valid Markdown on its own.
    pub text: Cow<'a, str>,
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

fn is_fence_line(trimmed: &str) -> bool {
    trimmed.starts_with("```") || trimmed.starts_with("~~~")
}

fn is_row_line(trimmed: &str) -> bool {
    trimmed.starts_with('|')
}

fn is_delimiter_line(trimmed: &str) -> bool {
    is_row_line(trimmed)
        && trimmed.contains('-')
        && trimmed
            .chars()
            .all(|c| matches!(c, '|' | '-' | ':' | ' ' | '\t'))
}

fn with_newline(line: &str) -> String {
    let mut l = line.to_string();
    if !l.ends_with('\n') {
        l.push('\n');
    }
    l
}

/// Markdown structure that is open at some point of a document.
#[derive(Debug, Default, Clone)]
struct Structure {
    /// Opening line of the fenced code block we are inside.
    fence: Option<String>,
    /// The previous line, if it was a table row (a possible header).
    prev_row: Option<String>,
    /// Header and delimiter rows of the table we are inside.
    table_header: Option<String>,
    /// Data rows seen in the current table.
    table_rows: usize,
}

impl Structure {
    fn feed(&mut self, line: &str) {
        let t = line.trim();
        if self.fence.is_some() {
            if is_fence_line(t) {
                self.fence = None;
            }
            return;
        }
        if is_fence_line(t) {
            *self = Structure {
                fence: Some(with_newline(line)),
                ..Structure::default()
            };
            return;
        }
        if !is_row_line(t) {
            self.prev_row = None;
            self.table_header = None;
            self.table_rows = 0;
            return;
        }
        if self.table_header.is_some() {
            self.table_rows += 1;
            return;
        }
        match self.prev_row.take() {
            Some(prev) if is_delimiter_line(t) => {
                self.table_header = Some(prev + &with_newline(line));
                self.table_rows = 0;
            }
            _ => self.prev_row = Some(with_newline(line)),
        }
    }

    /// Line that closes the open fence, if any.
    fn closer(&self) -> Option<String> {
        let opener = self.fence.as_deref()?.trim();
        let marker = opener.chars().next()?;
        let run = opener.chars().take_while(|&c| c == marker).count();
        Some(format!("{}\n", marker.to_string().repeat(run)))
    }

    /// Text that re-opens the structure for a chunk starting at `rest`.
    fn reopen(&self, rest: &str) -> String {
        if let Some(opener) = &self.fence {
            return opener.clone();
        }
        let continues_table = rest.lines().next().is_some_and(|l| is_row_line(l.trim()));
        match &self.table_header {
            Some(header) if continues_table => header.clone(),
            _ => String::new(),
        }
    }
}

/// Structure open at byte offset `cursor` (a line partly before `cursor`
/// does not count).
fn structure_at(text: &str, cursor: usize) -> Structure {
    let mut st = Structure::default();
    let mut pos = 0usize;
    for line in text.split_inclusive('\n') {
        if pos + line.len() > cursor {
            break;
        }
        st.feed(line);
        pos += line.len();
    }
    st
}

/// Length of the longest prefix of `line` within `budget` tokens, cut at
/// character boundaries; at least one character.
fn split_line_by_chars(line: &str, budget: usize) -> usize {
    let mut last = 0usize;
    for (i, c) in line.char_indices() {
        let next = i + c.len_utf8();
        if last > 0 && estimate_tokens(&line[..next]) > budget {
            break;
        }
        last = next;
    }
    last
}

/// Length of the largest prefix of `block` (cut between lines) that fits in
/// `budget` tokens, starting in structure `st`. Always makes progress: it
/// takes at least one line, never leaves a table header without a data row or
/// a code fence without content, and never cuts a table row. Only a plain
/// line or a code line longer than the budget is cut by characters.
fn split_oversized(block: &str, budget: usize, st: &Structure) -> usize {
    let mut end = 0usize;
    let mut state = st.clone();
    // The previous line opened a fence, so nothing has been taken from it yet.
    let mut at_opener = false;
    for line in block.split_inclusive('\n') {
        let mut after = state.clone();
        after.feed(line);
        let closer = after.closer().map_or(0, |c| estimate_tokens(&c));
        let over = estimate_tokens(&block[..end + line.len()]) + closer > budget;
        // Fence lines are atomic: a cut marker or language tag would not be
        // recognised as a fence, leaving the next chunk inside a broken opener.
        // The code after the opener is what gets cut by characters.
        if over && end == 0 && !is_row_line(line.trim()) && !is_fence_line(line.trim()) {
            // A single long line: cut it by characters.
            return split_line_by_chars(line, budget.saturating_sub(closer));
        }
        if over && end > 0 {
            if at_opener {
                if !is_fence_line(line.trim()) {
                    let used = estimate_tokens(&block[..end]) + closer;
                    return end + split_line_by_chars(line, budget.saturating_sub(used));
                }
            } else {
                let header_incomplete = (state.table_header.is_some() && state.table_rows == 0)
                    || (state.prev_row.is_some() && is_delimiter_line(line.trim()));
                if !header_incomplete {
                    break;
                }
            }
        }
        at_opener = after.fence.is_some() && state.fence.is_none();
        end += line.len();
        state = after;
    }
    end
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
            text: Cow::Borrowed(rest),
            tokens: estimate_tokens(rest),
            next_cursor: None,
            total_tokens,
        });
    };

    // A cursor inside a fence or table starts a chunk that must re-open it.
    let st = structure_at(text, cursor);
    let prefix = st.reopen(rest);
    let prefix_tokens = estimate_tokens(&prefix);

    let mut end = cursor;
    // Ranges come from the whole text so the fence state is right even when
    // the cursor is inside a fenced block.
    for (s, e) in block_ranges(text).into_iter().filter(|&(_, e)| e > cursor) {
        if prefix_tokens + estimate_tokens(&text[cursor..e]) <= max {
            end = e;
            continue;
        }
        if end == cursor {
            // The first block alone is over budget: split it.
            let s = s.max(cursor);
            end = s + split_oversized(&text[s..e], max.saturating_sub(prefix_tokens), &st);
        }
        break;
    }

    // Close a fence the split left open, unless only its closing line is left.
    let mut suffix = String::new();
    let open = structure_at(text, end);
    if let Some(closer) = open.closer() {
        let line_end = text[end..].find('\n').map(|i| end + i + 1);
        let at_line_start = end == 0 || text.as_bytes()[end - 1] == b'\n';
        match line_end {
            Some(le) if at_line_start && is_fence_line(text[end..le].trim()) => end = le,
            _ => suffix = closer,
        }
    }

    let body = &text[cursor..end];
    let chunk = if prefix.is_empty() && suffix.is_empty() {
        Cow::Borrowed(body)
    } else {
        let mut out = prefix;
        out.push_str(body);
        if !suffix.is_empty() {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(&suffix);
        }
        Cow::Owned(out)
    };
    let next = end;
    Ok(Chunk {
        tokens: estimate_tokens(&chunk),
        text: chunk,
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
            out.push_str(&c.text);
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

    /// All chunks of `doc` for a budget, following the cursors.
    fn all_chunks(doc: &str, budget: usize) -> Vec<String> {
        let mut cursor = 0;
        let mut out = Vec::new();
        loop {
            let c = take_chunk(doc, cursor, Some(budget)).unwrap();
            assert!(c.next_cursor.is_none_or(|n| n > cursor), "no progress");
            out.push(c.text.into_owned());
            match c.next_cursor {
                Some(n) => cursor = n,
                None => return out,
            }
        }
    }

    fn fence_lines(chunk: &str) -> usize {
        chunk.lines().filter(|l| l.trim() == "```").count()
    }

    #[test]
    fn oversized_code_block_chunks_stay_fenced() {
        let code: Vec<String> = (0..40).map(|i| format!("let value_{i} = {i};")).collect();
        let doc = format!("intro\n\n```rust\n{}\n```\n\nend\n", code.join("\n"));
        let chunks = all_chunks(&doc, 20);
        assert!(chunks.len() > 3, "{chunks:?}");
        let mut seen = Vec::new();
        for chunk in &chunks {
            let opens = chunk.lines().filter(|l| l.starts_with("```rust")).count();
            let closes = fence_lines(chunk);
            assert_eq!(opens, closes, "unbalanced fences in:\n{chunk}");
            seen.extend(
                chunk
                    .lines()
                    .filter(|l| l.starts_with("let value_"))
                    .map(String::from),
            );
        }
        assert_eq!(seen, code, "code lines lost or duplicated");
    }

    #[test]
    fn fence_opener_over_budget_is_never_cut() {
        let doc = "```rust\nabc\n```\n";
        for budget in 1..=6 {
            let chunks = all_chunks(doc, budget);
            let mut code = String::new();
            for chunk in &chunks {
                let lines: Vec<&str> = chunk.lines().collect();
                assert_eq!(lines.first(), Some(&"```rust"), "budget {budget}: {chunk:?}");
                assert_eq!(lines.last(), Some(&"```"), "budget {budget}: {chunk:?}");
                assert!(lines.len() > 2, "budget {budget}: empty block {chunk:?}");
                code.push_str(&lines[1..lines.len() - 1].concat());
            }
            assert_eq!(code, "abc", "budget {budget}: {chunks:?}");
        }
    }

    #[test]
    fn closing_fence_alone_is_not_a_chunk() {
        let doc = "```\naaaa aaaa\nbbbb bbbb\n```\n";
        for chunk in all_chunks(doc, 4) {
            assert!(
                chunk.lines().any(|l| l != "```"),
                "empty code block chunk: {chunk:?}"
            );
        }
    }

    #[test]
    fn oversized_table_repeats_the_header_and_keeps_rows_whole() {
        let rows: Vec<String> = (0..30).map(|i| format!("| row{i} | value{i} |")).collect();
        let doc = format!("| Name | Value |\n| --- | --- |\n{}\n", rows.join("\n"));
        let chunks = all_chunks(&doc, 25);
        assert!(chunks.len() > 3, "{chunks:?}");
        let mut seen = Vec::new();
        for chunk in &chunks {
            let mut lines = chunk.lines();
            assert_eq!(lines.next(), Some("| Name | Value |"), "{chunk}");
            assert_eq!(lines.next(), Some("| --- | --- |"), "{chunk}");
            for row in lines {
                assert!(
                    row.starts_with("| row") && row.ends_with(" |"),
                    "cut row: {row}"
                );
                seen.push(row.to_string());
            }
        }
        assert_eq!(seen, rows);
    }

    #[test]
    fn table_header_is_never_separated_from_its_first_row() {
        let doc =
            "| A very long header cell | Another long header cell |\n| --- | --- |\n| x | y |\n";
        let chunks = all_chunks(doc, 3);
        assert!(chunks[0].contains("| x | y |"), "{chunks:?}");
    }

    #[test]
    fn oversized_table_row_is_not_cut() {
        let long = "x".repeat(200);
        let doc = format!("| H |\n| --- |\n| {long} |\n| short |\n");
        let chunks = all_chunks(&doc, 10);
        assert!(
            chunks.iter().any(|c| c.contains(&format!("| {long} |"))),
            "{chunks:?}"
        );
    }

    #[test]
    fn text_after_a_table_gets_no_header() {
        let doc = "| H |\n| --- |\n| a |\n\nafter the table\n";
        let chunks = all_chunks(doc, 6);
        assert!(
            chunks.iter().any(|c| c.starts_with("after")),
            "header leaked into trailing text: {chunks:?}"
        );
    }
}
