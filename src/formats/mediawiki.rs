use std::io::Write;

use crate::converter::Converter;
use crate::document::{Block, Builder, Document, Inline};
use crate::error::{Error, Result};

pub struct MediaWikiConverter;

impl Converter for MediaWikiConverter {
    fn format_name(&self) -> &'static str {
        "mediawiki"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let text = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "mediawiki",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        parse_wiki(text).write_markdown(writer)
    }
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

fn strip_tag_span(s: &str, tag: &str) -> String {
    let open_full = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = String::new();
    let mut rest = s;
    loop {
        let Some(start) = rest.find(&open_full) else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let Some(gt) = rest[start..].find('>') else {
            break;
        };
        let tag_end = start + gt + 1;
        if rest[start..tag_end].ends_with("/>") {
            rest = &rest[tag_end..];
            continue;
        }
        match rest[tag_end..].find(&close) {
            Some(close_pos) => {
                rest = &rest[tag_end + close_pos + close.len()..];
            }
            None => {
                rest = &rest[tag_end..];
            }
        }
    }
    out
}

fn strip_templates(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '{' && chars[i + 1] == '{' {
            depth += 1;
            i += 2;
            continue;
        }
        if depth > 0 && i + 1 < chars.len() && chars[i] == '}' && chars[i + 1] == '}' {
            depth -= 1;
            i += 2;
            continue;
        }
        if depth == 0 {
            out.push(chars[i]);
        }
        i += 1;
    }
    out
}

/// Inline markup: bold and italic quotes, `<code>`, links, files and `<br>`.
/// Templates, `<ref>` notes and category links are dropped.
fn inline_wiki(s: &str) -> Vec<Inline> {
    let s = strip_tag_span(s, "ref");
    let s = strip_templates(&s);
    let s = s.replace("<nowiki>", "").replace("</nowiki>", "");
    parse_inline(&s.chars().collect::<Vec<_>>())
}

fn parse_inline(chars: &[char]) -> Vec<Inline> {
    let n = chars.len();
    let mut out = Vec::new();
    let mut text = String::new();
    let flush = |text: &mut String, out: &mut Vec<Inline>| {
        if !text.is_empty() {
            out.push(Inline::Text(std::mem::take(text)));
        }
    };
    let starts = |i: usize, s: &str| {
        s.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    let mut i = 0;
    while i < n {
        if starts(i, "[[")
            && let Some(close) = find_str(chars, i, "]]")
        {
            let inner: String = chars[i + 2..close].iter().collect();
            i = close + 2;
            let lower = inner.to_ascii_lowercase();
            if lower.starts_with("category:") {
                continue;
            }
            let (target, label) = match inner.split_once('|') {
                Some((t, l)) => (t.to_string(), l.to_string()),
                None => (inner.clone(), inner),
            };
            flush(&mut text, &mut out);
            if lower.starts_with("file:") || lower.starts_with("image:") {
                out.push(Inline::Image {
                    url: target,
                    alt: label,
                });
            } else {
                out.push(Inline::Link {
                    url: target,
                    content: vec![Inline::Text(label)],
                });
            }
            continue;
        }
        if chars[i] == '['
            && let Some(rel) = chars[i + 1..].iter().position(|&c| c == ']')
        {
            let close = i + 1 + rel;
            let inner: String = chars[i + 1..close].iter().collect();
            if inner.starts_with("http://") || inner.starts_with("https://") {
                let mut parts = inner.splitn(2, ' ');
                let url = parts.next().unwrap_or("").to_string();
                let label = parts.next().map(str::trim).unwrap_or_default();
                let label = if label.is_empty() {
                    url.clone()
                } else {
                    label.to_string()
                };
                flush(&mut text, &mut out);
                out.push(Inline::Link {
                    url,
                    content: vec![Inline::Text(label)],
                });
                i = close + 1;
                continue;
            }
        }
        if chars[i] == '\'' {
            let run = chars[i..].iter().take_while(|&&c| c == '\'').count();
            let (mark, wrap): (&str, fn(Vec<Inline>) -> Inline) = match run {
                5.. => ("'''''", |inner| {
                    Inline::Strong(vec![Inline::Emphasis(inner)])
                }),
                3 | 4 => ("'''", Inline::Strong),
                _ => ("''", Inline::Emphasis),
            };
            if run >= 2
                && let Some(close) = find_str(chars, i + mark.len(), mark)
            {
                flush(&mut text, &mut out);
                out.push(wrap(parse_inline(&chars[i + mark.len()..close])));
                i = close + mark.len();
                continue;
            }
            text.extend(std::iter::repeat_n('\'', run));
            i += run;
            continue;
        }
        if starts(i, "<code>")
            && let Some(close) = find_str(chars, i + 6, "</code>")
        {
            flush(&mut text, &mut out);
            out.push(Inline::Code(chars[i + 6..close].iter().collect()));
            i = close + 7;
            continue;
        }
        if let Some(len) = ["<br />", "<br/>", "<br>"]
            .iter()
            .find(|t| starts(i, t))
            .map(|t| t.len())
        {
            flush(&mut text, &mut out);
            out.push(Inline::Break);
            i += len;
            continue;
        }
        text.push(chars[i]);
        i += 1;
    }
    flush(&mut text, &mut out);
    out
}

fn parse_wiki_table(lines: &[&str], start: usize) -> (Vec<Vec<Vec<Inline>>>, usize) {
    let mut i = start + 1;
    let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut current_row: Vec<Vec<Inline>> = Vec::new();
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed.starts_with("|}") {
            i += 1;
            break;
        }
        if trimmed.starts_with("|-") {
            if !current_row.is_empty() {
                rows.push(std::mem::take(&mut current_row));
            }
            i += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('!') {
            for cell in rest.split("!!") {
                current_row.push(inline_wiki(cell.trim()));
            }
            i += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('|') {
            if !rest.starts_with('}') {
                for cell in rest.split("||") {
                    current_row.push(inline_wiki(cell.trim()));
                }
            }
            i += 1;
            continue;
        }
        i += 1;
    }
    if !current_row.is_empty() {
        rows.push(current_row);
    }
    (rows, i)
}

fn is_special_wiki_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('=')
        || t.starts_with('*')
        || t.starts_with('#')
        || t.starts_with(':')
        || t.starts_with("{|")
        || t.starts_with("<pre>")
        || (t.chars().all(|c| c == '-') && t.len() >= 4)
}

fn code(lines: Vec<String>) -> Block {
    Block::Code {
        lang: None,
        text: lines.join("\n"),
    }
}

fn parse_wiki(text: &str) -> Document {
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

        if trimmed.starts_with('=') {
            let eq_prefix = trimmed.chars().take_while(|&c| c == '=').count();
            let rest_after_prefix = trimmed[eq_prefix..].trim_end();
            let eq_suffix = rest_after_prefix
                .chars()
                .rev()
                .take_while(|&c| c == '=')
                .count();
            if eq_prefix > 0 && eq_suffix > 0 && rest_after_prefix.len() >= eq_suffix {
                let title = rest_after_prefix[..rest_after_prefix.len() - eq_suffix].trim();
                out.push(Block::Heading {
                    level: eq_prefix.min(6) as u8,
                    content: inline_wiki(title),
                });
                i += 1;
                continue;
            }
        }

        if trimmed.chars().all(|c| c == '-') && trimmed.len() >= 4 {
            out.push(Block::Rule);
            i += 1;
            continue;
        }

        if trimmed.starts_with("{|") {
            let (rows, next_i) = parse_wiki_table(&lines, i);
            out.push(Block::Table(rows));
            i = next_i;
            continue;
        }

        if let Some(after_open) = trimmed.strip_prefix("<pre>") {
            let mut block = Vec::new();
            if let Some(inline_content) = after_open.strip_suffix("</pre>") {
                if !inline_content.is_empty() {
                    block.push(inline_content.to_string());
                }
                out.push(code(block));
                i += 1;
                continue;
            }
            if !after_open.is_empty() {
                block.push(after_open.to_string());
            }
            i += 1;
            while i < lines.len() && !lines[i].contains("</pre>") {
                block.push(lines[i].to_string());
                i += 1;
            }
            if i < lines.len() {
                let before_close = lines[i].split("</pre>").next().unwrap_or("");
                if !before_close.is_empty() {
                    block.push(before_close.to_string());
                }
                i += 1;
            }
            out.push(code(block));
            continue;
        }

        if line.starts_with(' ') {
            let mut block = Vec::new();
            while i < lines.len() && lines[i].starts_with(' ') && !lines[i].trim().is_empty() {
                block.push(lines[i][1..].to_string());
                i += 1;
            }
            out.push(code(block));
            continue;
        }

        if trimmed.starts_with('*') || trimmed.starts_with('#') {
            let marker = trimmed.chars().next().unwrap();
            let depth = trimmed.chars().take_while(|&c| c == marker).count();
            out.item(
                depth - 1,
                marker == '#',
                inline_wiki(trimmed[depth..].trim()),
            );
            i += 1;
            continue;
        }

        if trimmed.starts_with(':') {
            let depth = trimmed.chars().take_while(|&c| c == ':').count();
            out.push(Block::Quote(vec![Block::Paragraph(inline_wiki(
                trimmed[depth..].trim(),
            ))]));
            i += 1;
            continue;
        }

        if trimmed.to_ascii_lowercase().starts_with("[[category:") {
            i += 1;
            continue;
        }

        let mut para = vec![trimmed.to_string()];
        i += 1;
        while i < lines.len() && !lines[i].trim().is_empty() && !is_special_wiki_line(lines[i]) {
            para.push(lines[i].trim().to_string());
            i += 1;
        }
        out.push(Block::Paragraph(inline_wiki(&para.join(" "))));
    }

    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_wiki(text: &str) -> String {
        let mut out = Vec::new();
        parse_wiki(text).write_markdown(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_headings() {
        let out = convert_wiki("== Section ==\nBody\n");
        assert!(out.starts_with("## Section\n\n"));
    }

    #[test]
    fn test_bold_italic() {
        let out = convert_wiki("'''bold''' and ''italic''\n");
        assert!(out.contains("**bold**"));
        assert!(out.contains("*italic*"));
    }

    #[test]
    fn test_internal_link() {
        assert_eq!(
            convert_wiki("[[Main Page|Home]]\n"),
            "[Home](<Main Page>)\n"
        );
    }

    #[test]
    fn test_external_link() {
        assert_eq!(
            convert_wiki("[https://example.com Example]\n"),
            "[Example](https://example.com)\n"
        );
    }

    #[test]
    fn test_lists() {
        assert_eq!(convert_wiki("* one\n* two\n"), "- one\n- two\n");
        assert_eq!(convert_wiki("# one\n# two\n"), "1. one\n2. two\n");
    }

    #[test]
    fn test_table() {
        let input = "{|\n|-\n! A !! B\n|-\n| 1 || 2\n|}\n";
        let out = convert_wiki(input);
        assert!(out.contains("| A | B |"));
        assert!(out.contains("| - | - |"));
        assert!(out.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_category_and_ref_stripped() {
        let out = convert_wiki("Text<ref>cite</ref> more.\n\n[[Category:Foo]]\n");
        assert!(!out.contains("cite"));
        assert!(!out.contains("Category"));
        assert!(out.contains("Text more."));
    }

    #[test]
    fn test_horizontal_rule() {
        assert!(convert_wiki("a\n\n----\n\nb\n").contains("---\n"));
    }

    #[test]
    fn test_nested_lists_and_following_paragraph() {
        let out = convert_wiki("* a\n** a1\n* b\n\nAfter\n");
        assert_eq!(out, "- a\n  - a1\n- b\n\nAfter\n");
    }

    #[test]
    fn test_unclosed_quotes_stay_literal_and_br_is_a_break() {
        assert_eq!(convert_wiki("it''s<br>done\n"), "it''s  \ndone\n");
    }

    #[test]
    fn test_file_link_is_an_image() {
        assert_eq!(convert_wiki("[[File:a.png|alt]]\n"), "![alt](File:a.png)\n");
    }

    #[test]
    fn test_converter_trait() {
        let converter = MediaWikiConverter;
        let mut buf = Vec::new();
        converter
            .convert(b"== Title ==\n\nHello.\n", &mut buf)
            .unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "## Title\n\nHello.\n");
        assert_eq!(converter.format_name(), "mediawiki");
        assert_eq!(converter.output_extension(), "md");
    }
}
