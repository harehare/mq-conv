use std::io::Write;

use crate::converter::Converter;
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

        writer.write_all(convert_org(text).as_bytes())?;
        Ok(())
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

/// Converts a single-character toggle marker (Org's `*bold*`, `/italic/`, etc.) into a Markdown wrapper.
fn convert_emphasis(s: &str, marker: char, open_wrap: &str, close_wrap: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == marker {
            let prev_ok = i == 0 || chars[i - 1].is_whitespace() || is_punct(chars[i - 1]);
            let next_ok = i + 1 < n && !chars[i + 1].is_whitespace();
            if prev_ok && next_ok {
                let mut j = i + 1;
                let mut found = None;
                while j < n {
                    if chars[j] == marker && !chars[j - 1].is_whitespace() {
                        let after_ok =
                            j + 1 == n || chars[j + 1].is_whitespace() || is_punct(chars[j + 1]);
                        if after_ok {
                            found = Some(j);
                            break;
                        }
                    }
                    j += 1;
                }
                if let Some(close) = found {
                    let content: String = chars[i + 1..close].iter().collect();
                    out.push_str(open_wrap);
                    out.push_str(&content);
                    out.push_str(close_wrap);
                    i = close + 1;
                    continue;
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn convert_org_links(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        if chars[i] == '[' && i + 1 < n && chars[i + 1] == '['
            && let Some(close) = find_str(&chars, i, "]]") {
                let inner: String = chars[i + 2..close].iter().collect();
                let (url, text) = if let Some(pos) = inner.find("][") {
                    (inner[..pos].to_string(), inner[pos + 2..].to_string())
                } else {
                    (inner.clone(), inner.clone())
                };
                out.push('[');
                out.push_str(&text);
                out.push_str("](");
                out.push_str(&url);
                out.push(')');
                i = close + 2;
                continue;
            }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn inline_org_to_md(s: &str) -> String {
    let s = convert_org_links(s);
    let s = convert_emphasis(&s, '*', "**", "**");
    let s = convert_emphasis(&s, '/', "*", "*");
    let s = convert_emphasis(&s, '~', "`", "`");
    let s = convert_emphasis(&s, '=', "`", "`");
    convert_emphasis(&s, '+', "~~", "~~")
}

fn render_table(rows: &[Vec<String>], out: &mut String) {
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if cols == 0 {
        return;
    }
    for (ri, row) in rows.iter().enumerate() {
        out.push('|');
        for c in 0..cols {
            out.push(' ');
            out.push_str(row.get(c).map(|s| s.as_str()).unwrap_or(""));
            out.push_str(" |");
        }
        out.push('\n');
        if ri == 0 {
            out.push('|');
            for _ in 0..cols {
                out.push_str(" --- |");
            }
            out.push('\n');
        }
    }
    out.push('\n');
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

fn convert_org(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
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
            out.push_str(&format!("# {}\n\n", inline_org_to_md(rest.trim())));
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
                let level = stars.min(6);
                let rest = trimmed[stars + 1..].trim();
                out.push_str(&format!("{} {}\n\n", "#".repeat(level), inline_org_to_md(rest)));
                i += 1;
                continue;
            }
        }

        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with("#+begin_src") {
            let lang = trimmed[11..].trim();
            out.push_str(&format!("```{lang}\n"));
            i += 1;
            while i < lines.len() && !lines[i].trim().to_ascii_lowercase().starts_with("#+end_src")
            {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
            }
            out.push_str("```\n\n");
            i += 1;
            continue;
        }
        if lower.starts_with("#+begin_example") {
            out.push_str("```\n");
            i += 1;
            while i < lines.len()
                && !lines[i].trim().to_ascii_lowercase().starts_with("#+end_example")
            {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
            }
            out.push_str("```\n\n");
            i += 1;
            continue;
        }
        if lower.starts_with("#+begin_quote") {
            i += 1;
            while i < lines.len()
                && !lines[i].trim().to_ascii_lowercase().starts_with("#+end_quote")
            {
                let l = lines[i];
                if l.trim().is_empty() {
                    out.push_str(">\n");
                } else {
                    out.push_str("> ");
                    out.push_str(&inline_org_to_md(l.trim()));
                    out.push('\n');
                }
                i += 1;
            }
            out.push('\n');
            i += 1;
            continue;
        }

        if trimmed.chars().all(|c| c == '-') && trimmed.len() >= 5 {
            out.push_str("---\n\n");
            i += 1;
            continue;
        }

        if trimmed.starts_with('|') {
            let mut rows: Vec<Vec<String>> = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('|') {
                let row_line = lines[i].trim();
                if row_line.chars().all(|c| c == '|' || c == '-' || c == '+') {
                    i += 1;
                    continue;
                }
                let cells: Vec<String> = row_line
                    .trim_matches('|')
                    .split('|')
                    .map(|c| inline_org_to_md(c.trim()))
                    .collect();
                rows.push(cells);
                i += 1;
            }
            render_table(&rows, &mut out);
            continue;
        }

        let indent = indent_of(line);
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            out.push_str(&"  ".repeat(indent / 2));
            out.push_str("- ");
            out.push_str(&inline_org_to_md(rest));
            out.push('\n');
            i += 1;
            continue;
        }
        if let Some(rest) = strip_enum_marker(trimmed) {
            out.push_str(&"  ".repeat(indent / 2));
            out.push_str("1. ");
            out.push_str(&inline_org_to_md(rest));
            out.push('\n');
            i += 1;
            continue;
        }

        let mut para = vec![trimmed.to_string()];
        i += 1;
        while i < lines.len() && !lines[i].trim().is_empty() && !is_special_org_line(lines[i]) {
            para.push(lines[i].trim().to_string());
            i += 1;
        }
        out.push_str(&inline_org_to_md(&para.join(" ")));
        out.push_str("\n\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_headings() {
        let input = "* Top\n** Sub\nText\n";
        let out = convert_org(input);
        assert!(out.starts_with("# Top\n\n"));
        assert!(out.contains("## Sub\n\n"));
    }

    #[test]
    fn test_title_keyword() {
        assert_eq!(convert_org("#+TITLE: Doc\n"), "# Doc\n\n");
    }

    #[test]
    fn test_code_block() {
        let input = "#+BEGIN_SRC rust\nfn main() {}\n#+END_SRC\n";
        assert_eq!(convert_org(input), "```rust\nfn main() {}\n```\n\n");
    }

    #[test]
    fn test_quote_block() {
        let input = "#+BEGIN_QUOTE\nHello there\n#+END_QUOTE\n";
        assert_eq!(convert_org(input), "> Hello there\n\n");
    }

    #[test]
    fn test_lists() {
        assert_eq!(convert_org("- one\n- two\n"), "- one\n- two\n");
        assert_eq!(convert_org("1. one\n2. two\n"), "1. one\n1. two\n");
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
        assert!(out.contains("| --- | --- |"));
        assert!(out.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_horizontal_rule() {
        assert!(convert_org("para\n\n-----\n\nmore\n").contains("---\n"));
    }

    #[test]
    fn test_converter_trait() {
        let converter = OrgConverter;
        let mut buf = Vec::new();
        converter.convert(b"* Title\n\nHello.\n", &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "# Title\n\nHello.\n\n");
        assert_eq!(converter.format_name(), "org");
        assert_eq!(converter.output_extension(), "md");
    }
}
