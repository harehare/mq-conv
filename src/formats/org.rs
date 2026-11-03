use std::io::Write;

use crate::converter::Converter;
use crate::document::{Block, Builder, Document, Inline};
use crate::error::{Error, Result};

pub struct OrgConverter;

impl Converter for OrgConverter {
    fn format_name(&self) -> &'static str {
        "org"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let text = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "org",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        parse_org(text).write_markdown(writer)
    }
}

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn strip_enum_marker(s: &str) -> Option<&str> {
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

fn find_str(chars: &[char], from: usize, needle: &str) -> Option<usize> {
    let needle_chars: Vec<char> = needle.chars().collect();
    let n = needle_chars.len();
    let mut i = from;
    while i + n <= chars.len() {
        if chars[i..i + n] == needle_chars[..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// The Markdown for an Org toggle marker around `content`.
fn toggled(marker: char, content: &str) -> Inline {
    match marker {
        '*' => Inline::Strong(inline_org(content)),
        '/' => Inline::Emphasis(inline_org(content)),
        '+' => Inline::Strike(inline_org(content)),
        _ => Inline::Code(content.to_string()),
    }
}

/// Inline markup: `*bold*`, `/italic/`, `~code~`, `=verbatim=`, `+strike+` and `[[url][text]]`.
fn inline_org(s: &str) -> Vec<Inline> {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Inline>| {
        if !text.is_empty() {
            out.push(Inline::Text(std::mem::take(text)));
        }
    };
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '['
            && i + 1 < n
            && chars[i + 1] == '['
            && let Some(close) = find_str(&chars, i, "]]")
        {
            let inner: String = chars[i + 2..close].iter().collect();
            let (url, label) = match inner.find("][") {
                Some(pos) => (inner[..pos].to_string(), inner[pos + 2..].to_string()),
                None => (inner.clone(), inner),
            };
            flush(&mut text, &mut out);
            out.push(Inline::Link {
                url,
                content: vec![Inline::Text(label)],
            });
            i = close + 2;
            continue;
        }
        if matches!(c, '*' | '/' | '~' | '=' | '+') {
            let prev_ok = i == 0 || chars[i - 1].is_whitespace() || is_punct(chars[i - 1]);
            let next_ok = i + 1 < n && !chars[i + 1].is_whitespace();
            if prev_ok && next_ok {
                let close = (i + 1..n).find(|&j| {
                    chars[j] == c
                        && !chars[j - 1].is_whitespace()
                        && (j + 1 == n || chars[j + 1].is_whitespace() || is_punct(chars[j + 1]))
                });
                if let Some(close) = close {
                    flush(&mut text, &mut out);
                    let content: String = chars[i + 1..close].iter().collect();
                    out.push(toggled(c, &content));
                    i = close + 1;
                    continue;
                }
            }
        }
        text.push(c);
        i += 1;
    }
    flush(&mut text, &mut out);
    out
}

fn is_special_org_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('*')
        || t.starts_with("#+")
        || t.starts_with('|')
        || t.starts_with("- ")
        || t.starts_with("+ ")
        || strip_enum_marker(t).is_some()
        || (t.chars().all(|c| c == '-') && t.len() >= 5)
}

/// Lines up to the closing `#+end_...` marker, and the index after it.
fn block_lines<'a>(lines: &[&'a str], start: usize, end_marker: &str) -> (Vec<&'a str>, usize) {
    let mut i = start;
    let mut body = Vec::new();
    while i < lines.len() && !lines[i].trim().to_ascii_lowercase().starts_with(end_marker) {
        body.push(lines[i]);
        i += 1;
    }
    (body, i + 1)
}

fn parse_org(text: &str) -> Document {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Builder::default();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if trimmed == ":PROPERTIES:" {
            i += 1;
            while i < lines.len() && lines[i].trim() != ":END:" {
                i += 1;
            }
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed
            .strip_prefix("#+TITLE:")
            .or_else(|| trimmed.strip_prefix("#+title:"))
        {
            out.push(Block::Heading {
                level: 1,
                content: inline_org(rest.trim()),
            });
            i += 1;
            continue;
        }
        if trimmed.starts_with("#+") && !trimmed.to_ascii_lowercase().starts_with("#+begin_") {
            i += 1;
            continue;
        }

        if trimmed.starts_with('*') {
            let stars = trimmed.chars().take_while(|&c| c == '*').count();
            let after = trimmed.as_bytes().get(stars).copied();
            if stars > 0 && after == Some(b' ') {
                out.push(Block::Heading {
                    level: stars.min(6) as u8,
                    content: inline_org(trimmed[stars + 1..].trim()),
                });
                i += 1;
                continue;
            }
        }

        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("#+begin_src") {
            let lang = trimmed[11..].trim();
            let (body, next) = block_lines(&lines, i + 1, "#+end_src");
            out.push(Block::Code {
                lang: (!lang.is_empty()).then(|| lang.to_string()),
                text: body.join("\n"),
            });
            i = next;
            continue;
        }
        if lower.starts_with("#+begin_example") {
            let (body, next) = block_lines(&lines, i + 1, "#+end_example");
            out.push(Block::Code {
                lang: None,
                text: body.join("\n"),
            });
            i = next;
            continue;
        }
        if lower.starts_with("#+begin_quote") {
            let (body, next) = block_lines(&lines, i + 1, "#+end_quote");
            let paragraphs = body
                .split(|l| l.trim().is_empty())
                .filter(|p| !p.is_empty())
                .map(|p| {
                    let joined: Vec<&str> = p.iter().map(|l| l.trim()).collect();
                    Block::Paragraph(inline_org(&joined.join(" ")))
                })
                .collect();
            out.push(Block::Quote(paragraphs));
            i = next;
            continue;
        }

        if trimmed.chars().all(|c| c == '-') && trimmed.len() >= 5 {
            out.push(Block::Rule);
            i += 1;
            continue;
        }

        if trimmed.starts_with('|') {
            let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('|') {
                let row_line = lines[i].trim();
                i += 1;
                if row_line.chars().all(|c| c == '|' || c == '-' || c == '+') {
                    continue;
                }
                rows.push(
                    row_line
                        .trim_matches('|')
                        .split('|')
                        .map(|c| inline_org(c.trim()))
                        .collect(),
                );
            }
            out.push(Block::Table(rows));
            continue;
        }

        let depth = indent_of(line) / 2;
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            out.item(depth, false, inline_org(rest));
            i += 1;
            continue;
        }
        if let Some(rest) = strip_enum_marker(trimmed) {
            out.item(depth, true, inline_org(rest));
            i += 1;
            continue;
        }

        let mut para = vec![trimmed.to_string()];
        i += 1;
        while i < lines.len() && !lines[i].trim().is_empty() && !is_special_org_line(lines[i]) {
            para.push(lines[i].trim().to_string());
            i += 1;
        }
        out.push(Block::Paragraph(inline_org(&para.join(" "))));
    }

    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_org(text: &str) -> String {
        let mut out = Vec::new();
        parse_org(text).write_markdown(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_headings() {
        let input = "* Top\n** Sub\nText\n";
        let out = convert_org(input);
        assert!(out.starts_with("# Top\n\n"));
        assert!(out.contains("## Sub\n\nText\n"));
    }

    #[test]
    fn test_title_keyword() {
        assert_eq!(convert_org("#+TITLE: Doc\n"), "# Doc\n");
    }

    #[test]
    fn test_code_block() {
        let input = "#+BEGIN_SRC rust\nfn main() {}\n#+END_SRC\n";
        assert_eq!(convert_org(input), "```rust\nfn main() {}\n```\n");
    }

    #[test]
    fn test_quote_block() {
        let input = "#+BEGIN_QUOTE\nHello there\n#+END_QUOTE\n";
        assert_eq!(convert_org(input), "> Hello there\n");
    }

    #[test]
    fn test_lists() {
        assert_eq!(convert_org("- one\n- two\n"), "- one\n- two\n");
        assert_eq!(convert_org("1. one\n2. two\n"), "1. one\n2. two\n");
    }

    #[test]
    fn test_inline_markup() {
        let out = convert_org("*bold* /italic/ ~code~ +gone+ [[https://x.io][link]]\n");
        assert!(out.contains("**bold**"));
        assert!(out.contains("*italic*"));
        assert!(out.contains("`code`"));
        assert!(out.contains("~~gone~~"));
        assert!(out.contains("[link](https://x.io)"));
    }

    #[test]
    fn test_table() {
        let input = "| A | B |\n|---+---|\n| 1 | 2 |\n";
        let out = convert_org(input);
        assert!(out.contains("| A | B |"));
        assert!(out.contains("| - | - |"));
        assert!(out.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_horizontal_rule() {
        assert!(convert_org("para\n\n-----\n\nmore\n").contains("---\n"));
    }

    #[test]
    fn test_nested_list_and_following_paragraph() {
        let out = convert_org("- a\n  - a1\n- b\n\nAfter\n");
        assert_eq!(out, "- a\n  - a1\n- b\n\nAfter\n");
    }

    #[test]
    fn test_links_are_not_read_as_emphasis() {
        let out = convert_org("see [[https://x.io/a/b/][a/b]] now\n");
        assert_eq!(out, "see [a/b](https://x.io/a/b/) now\n");
    }

    #[test]
    fn test_converter_trait() {
        let converter = OrgConverter;
        let mut buf = Vec::new();
        converter.convert(b"* Title\n\nHello.\n", &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "# Title\n\nHello.\n");
        assert_eq!(converter.format_name(), "org");
        assert_eq!(converter.output_extension(), "md");
    }
}
