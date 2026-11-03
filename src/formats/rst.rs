use std::io::Write;

use crate::converter::Converter;
use crate::document::{Block, Document, FlatItem, Inline};
use crate::error::{Error, Result};

pub struct RstConverter;

impl Converter for RstConverter {
    fn format_name(&self) -> &'static str {
        "rst"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let text = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "rst",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        parse_rst(text).write_markdown(writer)
    }
}

fn is_underline_char(c: char) -> bool {
    matches!(
        c,
        '=' | '-' | '~' | '^' | '"' | '\'' | '`' | '#' | '*' | '+' | '.' | ':' | '_'
    )
}

fn underline_char(line: &str) -> Option<char> {
    let trimmed = line.trim_end();
    let first = trimmed.chars().next()?;
    if !is_underline_char(first) {
        return None;
    }
    if trimmed.chars().all(|c| c == first) {
        Some(first)
    } else {
        None
    }
}

fn heading_level(order: &mut Vec<char>, ch: char) -> usize {
    if let Some(pos) = order.iter().position(|&c| c == ch) {
        (pos + 1).min(6)
    } else {
        order.push(ch);
        order.len().min(6)
    }
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn dedent(lines: &[String]) -> Vec<String> {
    let min_indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| indent_of(l))
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| {
            if l.chars().count() >= min_indent {
                l.chars().skip(min_indent).collect()
            } else {
                l.trim_start().to_string()
            }
        })
        .collect()
}

/// Gathers the dedented block indented past `base_indent` that follows a directive or literal-block marker.
fn collect_block(lines: &[&str], start: usize, base_indent: usize) -> (Vec<String>, usize) {
    let mut collected = Vec::new();
    let mut i = start;
    while i < lines.len() && lines[i].trim().is_empty() {
        i += 1;
    }
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() {
            collected.push(String::new());
            i += 1;
            continue;
        }
        if indent_of(line) > base_indent {
            collected.push(line.to_string());
            i += 1;
        } else {
            break;
        }
    }
    while collected
        .last()
        .map(|l| l.trim().is_empty())
        .unwrap_or(false)
    {
        collected.pop();
    }
    (dedent(&collected), i)
}

fn strip_enum_marker(s: &str) -> Option<&str> {
    if let Some(rest) = s.strip_prefix("#. ") {
        return Some(rest);
    }
    let mut end_digits = 0;
    for (idx, c) in s.char_indices() {
        if c.is_ascii_digit() {
            end_digits = idx + c.len_utf8();
        } else {
            break;
        }
    }
    if end_digits > 0 {
        let remainder = &s[end_digits..];
        if let Some(r) = remainder
            .strip_prefix(". ")
            .or_else(|| remainder.strip_prefix(") "))
        {
            return Some(r);
        }
    }
    None
}

fn find_seq(chars: &[char], from: usize, seq: &str) -> Option<usize> {
    let seq_chars: Vec<char> = seq.chars().collect();
    let n = seq_chars.len();
    let mut i = from;
    while i + n <= chars.len() {
        if chars[i..i + n] == seq_chars[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Inline markup: `**strong**`, `*emphasis*`, ``` ``code`` ``` and `` `text <url>`_ ``.
/// Roles such as `:func:` and other interpreted text keep only their content.
fn inline_rst(input: &str) -> Vec<Inline> {
    let chars: Vec<char> = input.chars().collect();
    let mut out = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    let flush = |text: &mut String, out: &mut Vec<Inline>| {
        if !text.is_empty() {
            out.push(Inline::Text(std::mem::take(text)));
        }
    };
    while i < chars.len() {
        // Strip role prefixes like :func:`foo` -> `foo`
        if chars[i] == ':'
            && let Some(end) = chars[i + 1..].iter().position(|&c| c == ':')
        {
            let role: String = chars[i + 1..i + 1 + end].iter().collect();
            if !role.is_empty()
                && role.chars().all(|c| c.is_alphanumeric() || c == '_')
                && i + 1 + end + 1 < chars.len()
                && chars[i + 1 + end + 1] == '`'
            {
                i += 1 + end + 1;
                continue;
            }
        }

        if chars[i] == '`' {
            if i + 1 < chars.len() && chars[i + 1] == '`' {
                if let Some(close) = find_seq(&chars, i + 2, "``") {
                    flush(&mut text, &mut out);
                    out.push(Inline::Code(chars[i + 2..close].iter().collect()));
                    i = close + 2;
                    continue;
                }
            } else if let Some(rel_end) = chars[i + 1..].iter().position(|&c| c == '`') {
                let close = i + 1 + rel_end;
                let content: String = chars[i + 1..close].iter().collect();
                let mut j = close + 1;
                while j < chars.len() && chars[j] == '_' {
                    j += 1;
                }
                flush(&mut text, &mut out);
                match content.rfind(" <").filter(|_| content.ends_with('>')) {
                    Some(lt) => out.push(Inline::Link {
                        url: content[lt + 2..content.len() - 1].to_string(),
                        content: vec![Inline::Text(content[..lt].to_string())],
                    }),
                    None => text.push_str(&content),
                }
                i = j;
                continue;
            }
        }

        if chars[i] == '*' {
            let strong = chars.get(i + 1) == Some(&'*');
            let mark = if strong { "**" } else { "*" };
            let start = i + mark.len();
            let opens = chars
                .get(start)
                .is_some_and(|c| !c.is_whitespace() && *c != '*');
            let after_word = i > 0 && chars[i - 1].is_alphanumeric();
            if opens
                && !after_word
                && let Some(close) = find_seq(&chars, start, mark)
                && close > start
                && !chars[close - 1].is_whitespace()
            {
                flush(&mut text, &mut out);
                let inner = vec![Inline::Text(chars[start..close].iter().collect())];
                out.push(if strong {
                    Inline::Strong(inner)
                } else {
                    Inline::Emphasis(inner)
                });
                i = close + mark.len();
                continue;
            }
        }

        text.push(chars[i]);
        i += 1;
    }
    flush(&mut text, &mut out);
    out
}

fn parse_list_table_rows(lines: &[&str]) -> Vec<Vec<Vec<Inline>>> {
    let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    for raw in lines {
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix("* - ") {
            rows.push(vec![inline_rst(rest.trim())]);
        } else if let Some(rest) = trimmed.strip_prefix("- ")
            && let Some(last) = rows.last_mut()
        {
            last.push(inline_rst(rest.trim()));
        }
    }
    rows
}

const ADMONITIONS: &[&str] = &[
    "note",
    "warning",
    "important",
    "tip",
    "caution",
    "attention",
    "danger",
    "error",
    "hint",
];

/// Collects blocks, flushing a run of list items as lists before any other block.
#[derive(Default)]
struct Builder {
    doc: Document,
    items: Vec<FlatItem>,
}

impl Builder {
    fn flush(&mut self) {
        let items = std::mem::take(&mut self.items);
        self.doc.extend(Block::lists(items));
    }

    fn push(&mut self, block: Block) {
        self.flush();
        self.doc.push(block);
    }

    fn item(&mut self, depth: usize, ordered: bool, text: &str) {
        self.items.push(FlatItem {
            depth,
            ordered,
            content: inline_rst(text),
        });
    }

    fn finish(mut self) -> Document {
        self.flush();
        self.doc
    }
}

fn parse_rst(text: &str) -> Document {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Builder::default();
    let mut i = 0;
    let mut heading_order: Vec<char> = Vec::new();

    while i < lines.len() {
        let line = lines[i];

        if line.trim().is_empty() {
            i += 1;
            continue;
        }

        if let Some(ch) = underline_char(line)
            && i + 2 < lines.len()
            && let Some(ch2) = underline_char(lines[i + 2])
            && ch2 == ch
            && !lines[i + 1].trim().is_empty()
            && line.trim().chars().count() >= lines[i + 1].trim().chars().count()
        {
            let title = lines[i + 1].trim();
            let level = heading_level(&mut heading_order, ch);
            out.push(Block::Heading {
                level: level as u8,
                content: inline_rst(title),
            });
            i += 3;
            continue;
        }

        if i + 1 < lines.len()
            && let Some(ch) = underline_char(lines[i + 1])
        {
            let title_len = line.trim_end().chars().count();
            let underline_len = lines[i + 1].trim_end().chars().count();
            if title_len > 0 && underline_len >= title_len {
                let level = heading_level(&mut heading_order, ch);
                out.push(Block::Heading {
                    level: level as u8,
                    content: inline_rst(line.trim()),
                });
                i += 2;
                continue;
            }
        }

        if underline_char(line).is_some() && line.trim().chars().count() >= 4 {
            out.push(Block::Rule);
            i += 1;
            continue;
        }

        let trimmed = line.trim_start();
        let base_indent = indent_of(line);

        if let Some(rest) = trimmed.strip_prefix(".. ") {
            if let Some(lang_part) = rest
                .strip_prefix("code-block::")
                .or_else(|| rest.strip_prefix("sourcecode::"))
            {
                let lang = lang_part.trim();
                let (block, next_i) = collect_block(&lines, i + 1, base_indent);
                out.push(Block::Code {
                    lang: (!lang.is_empty()).then(|| lang.to_string()),
                    text: block.join("\n"),
                });
                i = next_i;
                continue;
            }

            if rest.strip_prefix("list-table::").is_some() {
                let (block, next_i) = collect_block(&lines, i + 1, base_indent);
                let rows_lines: Vec<&str> = block
                    .iter()
                    .map(|s| s.as_str())
                    .filter(|l| !l.trim_start().starts_with(':'))
                    .collect();
                out.push(Block::Table(parse_list_table_rows(&rows_lines)));
                i = next_i;
                continue;
            }

            let admonition = ADMONITIONS.iter().find_map(|adm| {
                rest.strip_prefix(&format!("{adm}::"))
                    .map(|after| (*adm, after))
            });
            if let Some((adm, after)) = admonition {
                let (block, next_i) = collect_block(&lines, i + 1, base_indent);
                let mut label_chars = adm.chars();
                let label = match label_chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + label_chars.as_str(),
                    None => String::new(),
                };
                let mut first = vec![Inline::Strong(vec![Inline::Text(format!("{label}:"))])];
                let after_trim = after.trim();
                if !after_trim.is_empty() {
                    first.push(Inline::Text(" ".to_string()));
                    first.extend(inline_rst(after_trim));
                }
                let mut quote = vec![Block::Paragraph(first)];
                for paragraph in block.split(|l| l.trim().is_empty()) {
                    if !paragraph.is_empty() {
                        quote.push(Block::Paragraph(inline_rst(&paragraph.join(" "))));
                    }
                }
                out.push(Block::Quote(quote));
                i = next_i;
                continue;
            }

            if let Some(path) = rest
                .strip_prefix("image::")
                .or_else(|| rest.strip_prefix("figure::"))
            {
                out.push(Block::Paragraph(vec![Inline::Image {
                    url: path.trim().to_string(),
                    alt: String::new(),
                }]));
                let (_block, next_i) = collect_block(&lines, i + 1, base_indent);
                i = next_i;
                continue;
            }

            // Unrecognized directive or comment: drop the marker and its block
            let (_block, next_i) = collect_block(&lines, i + 1, base_indent);
            i = next_i;
            continue;
        }

        if let Some(rest) = trimmed
            .strip_prefix("* ")
            .or_else(|| trimmed.strip_prefix("- "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            out.item(base_indent / 3, false, rest.trim());
            i += 1;
            continue;
        }

        if let Some(rest) = strip_enum_marker(trimmed) {
            out.item(base_indent / 3, true, rest.trim());
            i += 1;
            continue;
        }

        // Literal block introduced by a trailing "::"
        if let Some(stripped) = line.trim_end().strip_suffix("::") {
            let (block, next_i) = collect_block(&lines, i + 1, base_indent);
            if !block.is_empty() {
                let text = stripped.trim();
                if !text.is_empty() {
                    let mut content = inline_rst(text);
                    content.push(Inline::Text(":".to_string()));
                    out.push(Block::Paragraph(content));
                }
                out.push(Block::Code {
                    lang: None,
                    text: block.join("\n"),
                });
                i = next_i;
                continue;
            }
        }

        // Default: paragraph (indented paragraphs become blockquotes)
        let mut para_lines = vec![line.trim().to_string()];
        i += 1;
        while i < lines.len()
            && !lines[i].trim().is_empty()
            && underline_char(lines[i]).is_none()
            && indent_of(lines[i]) == base_indent
        {
            para_lines.push(lines[i].trim().to_string());
            i += 1;
        }
        let paragraph = Block::Paragraph(inline_rst(&para_lines.join(" ")));
        out.push(if base_indent > 0 {
            Block::Quote(vec![paragraph])
        } else {
            paragraph
        });
    }

    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn convert_rst(text: &str) -> String {
        let mut out = Vec::new();
        parse_rst(text).write_markdown(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[rstest]
    #[case("Title\n=====\n\nBody text.\n", "# Title\n\nBody text.\n")]
    fn test_heading(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert_rst(input), expected);
    }

    #[test]
    fn test_nested_headings_use_distinct_underlines() {
        let input = "Top\n===\n\nSub\n---\n\nText\n";
        let out = convert_rst(input);
        assert!(out.contains("# Top"));
        assert!(out.contains("## Sub"));
    }

    #[test]
    fn test_code_block_directive() {
        let input = ".. code-block:: rust\n\n   fn main() {}\n";
        let out = convert_rst(input);
        assert_eq!(out, "```rust\nfn main() {}\n```\n");
    }

    #[test]
    fn test_literal_block() {
        let input = "Example::\n\n    foo bar\n";
        let out = convert_rst(input);
        assert_eq!(out, "Example:\n\n```\nfoo bar\n```\n");
    }

    #[test]
    fn test_bullet_list() {
        let input = "* one\n* two\n";
        assert_eq!(convert_rst(input), "- one\n- two\n");
    }

    #[test]
    fn test_enumerated_list() {
        let input = "#. one\n#. two\n";
        assert_eq!(convert_rst(input), "1. one\n2. two\n");
    }

    #[test]
    fn test_inline_markup() {
        let input = "**bold** and *em* and ``code`` and `link <https://x.io>`_\n";
        let out = convert_rst(input);
        assert!(out.contains("**bold**"));
        assert!(out.contains("*em*"));
        assert!(out.contains("`code`"));
        assert!(out.contains("[link](https://x.io)"));
    }

    #[test]
    fn test_list_table() {
        let input =
            ".. list-table::\n   :header-rows: 1\n\n   * - A\n     - B\n   * - 1\n     - 2\n";
        let out = convert_rst(input);
        assert!(out.contains("| A | B |"));
        assert!(out.contains("| - | - |"));
        assert!(out.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_note_admonition() {
        let input = ".. note::\n\n   Something important.\n";
        let out = convert_rst(input);
        assert!(out.starts_with("> **Note:**"));
        assert!(out.contains("> Something important."));
    }

    #[test]
    fn test_horizontal_rule() {
        let input = "para one\n\n----\n\npara two\n";
        let out = convert_rst(input);
        assert!(out.contains("---\n"));
    }

    #[test]
    fn test_list_is_followed_by_a_blank_line() {
        assert_eq!(convert_rst("* one\n* two\n\nAfter.\n"), "- one\n- two\n\nAfter.\n");
    }

    #[test]
    fn test_nested_lists() {
        let input = "* one\n\n   #. a\n   #. b\n\n* two\n";
        assert_eq!(convert_rst(input), "- one\n  1. a\n  2. b\n- two\n");
    }

    #[test]
    fn test_unmatched_asterisk_is_kept_as_text() {
        assert_eq!(convert_rst("2 * 3 = 6\n"), "2 \\* 3 = 6\n");
    }

    #[test]
    fn test_converter_trait() {
        let converter = RstConverter;
        let mut buf = Vec::new();
        converter
            .convert(b"Title\n=====\n\nHello.\n", &mut buf)
            .unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(out, "# Title\n\nHello.\n");
        assert_eq!(converter.format_name(), "rst");
        assert_eq!(converter.output_extension(), "md");
    }
}
