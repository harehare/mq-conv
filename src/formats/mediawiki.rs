use std::io::Write;

use crate::converter::Converter;
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

        writer.write_all(convert_wiki(text).as_bytes())?;
        Ok(())
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

fn convert_wiki_links(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        if chars[i] == '[' && i + 1 < n && chars[i + 1] == '['
            && let Some(close) = find_str(&chars, i, "]]") {
                let inner: String = chars[i + 2..close].iter().collect();
                let lower = inner.to_ascii_lowercase();
                if lower.starts_with("category:") {
                    i = close + 2;
                    continue;
                }
                let is_file = lower.starts_with("file:") || lower.starts_with("image:");
                let (target, text) = if let Some(pos) = inner.find('|') {
                    (inner[..pos].to_string(), inner[pos + 1..].to_string())
                } else {
                    (inner.clone(), inner.clone())
                };
                if is_file {
                    out.push_str(&format!("![{text}]({target})"));
                } else {
                    out.push_str(&format!("[{text}]({target})"));
                }
                i = close + 2;
                continue;
            }
        if chars[i] == '['
            && let Some(rel) = chars[i + 1..].iter().position(|&c| c == ']') {
                let close = i + 1 + rel;
                let inner: String = chars[i + 1..close].iter().collect();
                if inner.starts_with("http://") || inner.starts_with("https://") {
                    let mut parts = inner.splitn(2, ' ');
                    let url = parts.next().unwrap_or("").to_string();
                    let text = parts.next().map(|t| t.trim().to_string()).unwrap_or_default();
                    let text = if text.is_empty() { url.clone() } else { text };
                    out.push_str(&format!("[{text}]({url})"));
                    i = close + 1;
                    continue;
                }
            }
        out.push(chars[i]);
        i += 1;
    }
    out
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

fn inline_wiki_to_md(s: &str) -> String {
    let s = strip_tag_span(s, "ref");
    let s = strip_templates(&s);
    let s = convert_wiki_links(&s);
    let s = s.replace("'''''", "***").replace("'''", "**").replace("''", "*");
    let s = s.replace("<code>", "`").replace("</code>", "`");
    let s = s.replace("<nowiki>", "").replace("</nowiki>", "");
    s.replace("<br />", "  \n")
        .replace("<br/>", "  \n")
        .replace("<br>", "  \n")
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

fn parse_wiki_table(lines: &[&str], start: usize) -> (Vec<Vec<String>>, usize) {
    let mut i = start + 1;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
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
                current_row.push(inline_wiki_to_md(cell.trim()));
            }
            i += 1;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('|') {
            if !rest.starts_with('}') {
                for cell in rest.split("||") {
                    current_row.push(inline_wiki_to_md(cell.trim()));
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

fn convert_wiki(text: &str) -> String {
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

        if trimmed.starts_with('=') {
            let eq_prefix = trimmed.chars().take_while(|&c| c == '=').count();
            let rest_after_prefix = trimmed[eq_prefix..].trim_end();
            let eq_suffix = rest_after_prefix.chars().rev().take_while(|&c| c == '=').count();
            if eq_prefix > 0 && eq_suffix > 0 && rest_after_prefix.len() >= eq_suffix {
                let title = rest_after_prefix[..rest_after_prefix.len() - eq_suffix].trim();
                let level = eq_prefix.min(6);
                out.push_str(&format!("{} {}\n\n", "#".repeat(level), inline_wiki_to_md(title)));
                i += 1;
                continue;
            }
        }

        if trimmed.chars().all(|c| c == '-') && trimmed.len() >= 4 {
            out.push_str("---\n\n");
            i += 1;
            continue;
        }

        if trimmed.starts_with("{|") {
            let (rows, next_i) = parse_wiki_table(&lines, i);
            render_table(&rows, &mut out);
            i = next_i;
            continue;
        }

        if let Some(after_open) = trimmed.strip_prefix("<pre>") {
            let mut block = Vec::new();
            if let Some(inline_content) = after_open.strip_suffix("</pre>") {
                if !inline_content.is_empty() {
                    block.push(inline_content.to_string());
                }
                out.push_str("```\n");
                for l in &block {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push_str("```\n\n");
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
            out.push_str("```\n");
            for l in &block {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str("```\n\n");
            continue;
        }

        if line.starts_with(' ') {
            let mut block = Vec::new();
            while i < lines.len() && lines[i].starts_with(' ') && !lines[i].trim().is_empty() {
                block.push(lines[i][1..].to_string());
                i += 1;
            }
            out.push_str("```\n");
            for l in &block {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str("```\n\n");
            continue;
        }

        if trimmed.starts_with('*') || trimmed.starts_with('#') {
            let marker = trimmed.chars().next().unwrap();
            let depth = trimmed.chars().take_while(|&c| c == marker).count();
            let rest = trimmed[depth..].trim();
            out.push_str(&"  ".repeat(depth.saturating_sub(1)));
            if marker == '#' {
                out.push_str("1. ");
            } else {
                out.push_str("- ");
            }
            out.push_str(&inline_wiki_to_md(rest));
            out.push('\n');
            i += 1;
            continue;
        }

        if trimmed.starts_with(':') {
            let depth = trimmed.chars().take_while(|&c| c == ':').count();
            let rest = trimmed[depth..].trim();
            out.push_str("> ");
            out.push_str(&inline_wiki_to_md(rest));
            out.push('\n');
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
        out.push_str(&inline_wiki_to_md(&para.join(" ")));
        out.push_str("\n\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(convert_wiki("[[Main Page|Home]]\n"), "[Home](Main Page)\n\n");
    }

    #[test]
    fn test_external_link() {
        assert_eq!(
            convert_wiki("[https://example.com Example]\n"),
            "[Example](https://example.com)\n\n"
        );
    }

    #[test]
    fn test_lists() {
        assert_eq!(convert_wiki("* one\n* two\n"), "- one\n- two\n");
        assert_eq!(convert_wiki("# one\n# two\n"), "1. one\n1. two\n");
    }

    #[test]
    fn test_table() {
        let input = "{|\n|-\n! A !! B\n|-\n| 1 || 2\n|}\n";
        let out = convert_wiki(input);
        assert!(out.contains("| A | B |"));
        assert!(out.contains("| --- | --- |"));
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
    fn test_converter_trait() {
        let converter = MediaWikiConverter;
        let mut buf = Vec::new();
        converter.convert(b"== Title ==\n\nHello.\n", &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "## Title\n\nHello.\n\n");
        assert_eq!(converter.format_name(), "mediawiki");
        assert_eq!(converter.output_extension(), "md");
    }
}
