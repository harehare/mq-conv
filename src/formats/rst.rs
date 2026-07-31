use std::io::Write;

use crate::converter::Converter;
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

        writer.write_all(convert_rst(text).as_bytes())?;
        Ok(())
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
    while collected.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
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

fn inline_rst_to_md(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        // Strip role prefixes like :func:`foo` -> `foo`
        if chars[i] == ':'
            && let Some(end) = chars[i + 1..].iter().position(|&c| c == ':') {
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
                    let content: String = chars[i + 2..close].iter().collect();
                    out.push('`');
                    out.push_str(&content);
                    out.push('`');
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
                if let Some(lt) = content.rfind(" <")
                    && content.ends_with('>') {
                        let text = &content[..lt];
                        let url = &content[lt + 2..content.len() - 1];
                        out.push('[');
                        out.push_str(text);
                        out.push_str("](");
                        out.push_str(url);
                        out.push(')');
                        i = j;
                        continue;
                    }
                out.push_str(&content);
                i = j;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }
    out
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

fn parse_list_table_rows(lines: &[&str]) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    for raw in lines {
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix("* - ") {
            rows.push(vec![inline_rst_to_md(rest.trim())]);
        } else if let Some(rest) = trimmed.strip_prefix("- ")
            && let Some(last) = rows.last_mut() {
                last.push(inline_rst_to_md(rest.trim()));
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

fn convert_rst(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
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
                        out.push_str(&format!(
                            "{} {}\n\n",
                            "#".repeat(level),
                            inline_rst_to_md(title)
                        ));
                        i += 3;
                        continue;
                    }

        if i + 1 < lines.len()
            && let Some(ch) = underline_char(lines[i + 1]) {
                let title_len = line.trim_end().chars().count();
                let underline_len = lines[i + 1].trim_end().chars().count();
                if title_len > 0 && underline_len >= title_len {
                    let level = heading_level(&mut heading_order, ch);
                    out.push_str(&format!(
                        "{} {}\n\n",
                        "#".repeat(level),
                        inline_rst_to_md(line.trim())
                    ));
                    i += 2;
                    continue;
                }
            }

        if underline_char(line).is_some() && line.trim().chars().count() >= 4 {
            out.push_str("---\n\n");
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
                out.push_str(&format!("```{lang}\n"));
                for l in &block {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push_str("```\n\n");
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
                let rows = parse_list_table_rows(&rows_lines);
                render_table(&rows, &mut out);
                i = next_i;
                continue;
            }

            let admonition = ADMONITIONS
                .iter()
                .find_map(|adm| rest.strip_prefix(&format!("{adm}::")).map(|after| (*adm, after)));
            if let Some((adm, after)) = admonition {
                let (block, next_i) = collect_block(&lines, i + 1, base_indent);
                let mut label_chars = adm.chars();
                let label = match label_chars.next() {
                    Some(c) => c.to_uppercase().collect::<String>() + label_chars.as_str(),
                    None => String::new(),
                };
                out.push_str(&format!("> **{label}:**"));
                let after_trim = after.trim();
                if !after_trim.is_empty() {
                    out.push(' ');
                    out.push_str(&inline_rst_to_md(after_trim));
                }
                out.push('\n');
                for l in &block {
                    if l.trim().is_empty() {
                        out.push_str(">\n");
                    } else {
                        out.push_str("> ");
                        out.push_str(&inline_rst_to_md(l));
                        out.push('\n');
                    }
                }
                out.push('\n');
                i = next_i;
                continue;
            }

            if let Some(path) = rest
                .strip_prefix("image::")
                .or_else(|| rest.strip_prefix("figure::"))
            {
                out.push_str(&format!("![]({})\n\n", path.trim()));
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
            let depth = base_indent / 3;
            out.push_str(&"  ".repeat(depth));
            out.push_str("- ");
            out.push_str(&inline_rst_to_md(rest.trim()));
            out.push('\n');
            i += 1;
            continue;
        }

        if let Some(rest) = strip_enum_marker(trimmed) {
            let depth = base_indent / 3;
            out.push_str(&"  ".repeat(depth));
            out.push_str("1. ");
            out.push_str(&inline_rst_to_md(rest.trim()));
            out.push('\n');
            i += 1;
            continue;
        }

        // Literal block introduced by a trailing "::"
        if let Some(stripped) = line.trim_end().strip_suffix("::") {
            let (block, next_i) = collect_block(&lines, i + 1, base_indent);
            if !block.is_empty() {
                let text = stripped.trim();
                if !text.is_empty() {
                    out.push_str(&inline_rst_to_md(text));
                    out.push_str(":\n\n");
                }
                out.push_str("```\n");
                for l in &block {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push_str("```\n\n");
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
        let para = para_lines.join(" ");
        if base_indent > 0 {
            out.push_str("> ");
        }
        out.push_str(&inline_rst_to_md(&para));
        out.push_str("\n\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("Title\n=====\n\nBody text.\n", "# Title\n\nBody text.\n\n")]
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
        assert_eq!(out, "```rust\nfn main() {}\n```\n\n");
    }

    #[test]
    fn test_literal_block() {
        let input = "Example::\n\n    foo bar\n";
        let out = convert_rst(input);
        assert_eq!(out, "Example:\n\n```\nfoo bar\n```\n\n");
    }

    #[test]
    fn test_bullet_list() {
        let input = "* one\n* two\n";
        assert_eq!(convert_rst(input), "- one\n- two\n");
    }

    #[test]
    fn test_enumerated_list() {
        let input = "#. one\n#. two\n";
        assert_eq!(convert_rst(input), "1. one\n1. two\n");
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
        let input = ".. list-table::\n   :header-rows: 1\n\n   * - A\n     - B\n   * - 1\n     - 2\n";
        let out = convert_rst(input);
        assert!(out.contains("| A | B |"));
        assert!(out.contains("| --- | --- |"));
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
    fn test_converter_trait() {
        let converter = RstConverter;
        let mut buf = Vec::new();
        converter
            .convert(b"Title\n=====\n\nHello.\n", &mut buf)
            .unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(out, "# Title\n\nHello.\n\n");
        assert_eq!(converter.format_name(), "rst");
        assert_eq!(converter.output_extension(), "md");
    }
}
