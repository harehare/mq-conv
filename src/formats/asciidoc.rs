use std::io::Write;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct AsciidocConverter;

impl Converter for AsciidocConverter {
    fn format_name(&self) -> &'static str {
        "asciidoc"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let text = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "asciidoc",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        writer.write_all(convert_adoc(text).as_bytes())?;
        Ok(())
    }
}

fn is_delim(line: &str, ch: char, min_len: usize) -> bool {
    line.len() >= min_len && !line.is_empty() && line.chars().all(|c| c == ch)
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Converts a single-character toggle marker (AsciiDoc's `*bold*`, `_italic_`) into a Markdown wrapper.
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

fn convert_adoc_links(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out = String::new();
    let mut i = 0;
    while i < n {
        let starts_with = |from: usize, lit: &str| -> bool {
            let lc: Vec<char> = lit.chars().collect();
            from + lc.len() <= n && chars[from..from + lc.len()] == lc[..]
        };
        let prefix_len = if starts_with(i, "link:") { 5 } else { 0 };
        let is_url = starts_with(i, "http://") || starts_with(i, "https://");
        if prefix_len > 0 || is_url {
            let url_start = i + prefix_len;
            if let Some(rel) = chars[url_start..]
                .iter()
                .position(|&c| c == '[' || c.is_whitespace())
                && chars[url_start + rel] == '[' {
                    let bracket_pos = url_start + rel;
                    let url: String = chars[url_start..bracket_pos].iter().collect();
                    if let Some(close_rel) = chars[bracket_pos + 1..].iter().position(|&c| c == ']') {
                        let close = bracket_pos + 1 + close_rel;
                        let text: String = chars[bracket_pos + 1..close].iter().collect();
                        let text = if text.is_empty() { url.clone() } else { text };
                        out.push('[');
                        out.push_str(&text);
                        out.push_str("](");
                        out.push_str(&url);
                        out.push(')');
                        i = close + 1;
                        continue;
                    }
                }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn inline_adoc_to_md(s: &str) -> String {
    let s = convert_adoc_links(s);
    let s = convert_emphasis(&s, '*', "**", "**");
    convert_emphasis(&s, '_', "*", "*")
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

fn collect_delim_block(lines: &[&str], start: usize, delim_char: char) -> (Vec<String>, usize) {
    let mut i = start;
    let mut block = Vec::new();
    while i < lines.len() && !is_delim(lines[i].trim(), delim_char, 4) {
        block.push(lines[i].to_string());
        i += 1;
    }
    if i < lines.len() {
        i += 1;
    }
    (block, i)
}

fn parse_adoc_table(
    lines: &[&str],
    start: usize,
    col_hint: Option<usize>,
) -> (Vec<Vec<String>>, usize) {
    let mut i = start + 1;
    let mut cells: Vec<String> = Vec::new();
    let mut first_row_break: Option<usize> = None;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == "|===" {
            i += 1;
            break;
        }
        if trimmed.is_empty() {
            if !cells.is_empty() && first_row_break.is_none() {
                first_row_break = Some(cells.len());
            }
            i += 1;
            continue;
        }
        for part in trimmed.split('|') {
            let p = part.trim();
            if !p.is_empty() {
                cells.push(inline_adoc_to_md(p));
            }
        }
        i += 1;
    }
    let col_count = col_hint
        .or(first_row_break)
        .unwrap_or_else(|| cells.len().max(1))
        .max(1);
    let rows: Vec<Vec<String>> = cells.chunks(col_count).map(|c| c.to_vec()).collect();
    (rows, i)
}

const ADMONITIONS: &[&str] = &["NOTE", "TIP", "WARNING", "IMPORTANT", "CAUTION"];

fn is_special_adoc_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('=')
        || t.starts_with('*')
        || t.starts_with('[')
        || t.starts_with("|===")
        || t.starts_with("image::")
        || t.starts_with("//")
        || is_delim(t, '-', 4)
        || is_delim(t, '.', 4)
        || is_delim(t, '_', 4)
        || is_delim(t, '/', 4)
        || t == "'''"
        || (t.starts_with('.') && t.len() > 1 && t.as_bytes()[1] == b' ')
}

fn convert_adoc(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut col_hint: Option<usize> = None;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if trimmed.starts_with("//") && !is_delim(trimmed, '/', 4) {
            i += 1;
            continue;
        }
        if is_delim(trimmed, '/', 4) {
            i += 1;
            while i < lines.len() && !is_delim(lines[i].trim(), '/', 4) {
                i += 1;
            }
            if i < lines.len() {
                i += 1;
            }
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix(':')
            && let Some(colon_pos) = rest.find(':') {
                let name = &rest[..colon_pos];
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '!')
                {
                    i += 1;
                    continue;
                }
            }

        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            let inner = &trimmed[1..trimmed.len() - 1];

            if let Some(pos) = inner.find("cols=\"") {
                let rest = &inner[pos + 6..];
                if let Some(end) = rest.find('"') {
                    col_hint = Some(rest[..end].split(',').count().max(1));
                }
            }

            let upper = inner.trim().to_ascii_uppercase();
            if ADMONITIONS.contains(&upper.as_str())
                && i + 1 < lines.len()
                && is_delim(lines[i + 1].trim(), '=', 4)
            {
                let label = capitalize(&upper.to_ascii_lowercase());
                i += 2;
                out.push_str(&format!("> **{label}:**\n"));
                while i < lines.len() && !is_delim(lines[i].trim(), '=', 4) {
                    let l = lines[i];
                    if l.trim().is_empty() {
                        out.push_str(">\n");
                    } else {
                        out.push_str("> ");
                        out.push_str(&inline_adoc_to_md(l.trim()));
                        out.push('\n');
                    }
                    i += 1;
                }
                if i < lines.len() {
                    i += 1;
                }
                out.push('\n');
                continue;
            }

            if upper.starts_with("QUOTE") && i + 1 < lines.len() && is_delim(lines[i + 1].trim(), '_', 4) {
                i += 2;
                while i < lines.len() && !is_delim(lines[i].trim(), '_', 4) {
                    let l = lines[i];
                    if l.trim().is_empty() {
                        out.push_str(">\n");
                    } else {
                        out.push_str("> ");
                        out.push_str(&inline_adoc_to_md(l.trim()));
                        out.push('\n');
                    }
                    i += 1;
                }
                if i < lines.len() {
                    i += 1;
                }
                out.push('\n');
                continue;
            }

            if let Some(rest) = inner.strip_prefix("source") {
                let lang = rest.trim_start_matches(',').trim();
                if i + 1 < lines.len() && is_delim(lines[i + 1].trim(), '-', 4) {
                    let (block, next_i) = collect_delim_block(&lines, i + 2, '-');
                    out.push_str(&format!("```{lang}\n"));
                    for l in &block {
                        out.push_str(l);
                        out.push('\n');
                    }
                    out.push_str("```\n\n");
                    i = next_i;
                    continue;
                }
            }

            i += 1;
            continue;
        }

        if trimmed.starts_with('=') {
            let eq = trimmed.chars().take_while(|&c| c == '=').count();
            if (1..=6).contains(&eq) && trimmed.as_bytes().get(eq) == Some(&b' ') {
                let title = trimmed[eq + 1..].trim();
                out.push_str(&format!("{} {}\n\n", "#".repeat(eq), inline_adoc_to_md(title)));
                i += 1;
                continue;
            }
        }

        if is_delim(trimmed, '-', 4) {
            let (block, next_i) = collect_delim_block(&lines, i + 1, '-');
            out.push_str("```\n");
            for l in &block {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str("```\n\n");
            i = next_i;
            continue;
        }
        if is_delim(trimmed, '.', 4) {
            let (block, next_i) = collect_delim_block(&lines, i + 1, '.');
            out.push_str("```\n");
            for l in &block {
                out.push_str(l);
                out.push('\n');
            }
            out.push_str("```\n\n");
            i = next_i;
            continue;
        }
        if is_delim(trimmed, '_', 4) {
            let (block, next_i) = collect_delim_block(&lines, i + 1, '_');
            for l in &block {
                if l.trim().is_empty() {
                    out.push_str(">\n");
                } else {
                    out.push_str("> ");
                    out.push_str(&inline_adoc_to_md(l));
                    out.push('\n');
                }
            }
            out.push('\n');
            i = next_i;
            continue;
        }

        if trimmed.starts_with("|===") {
            let (rows, next_i) = parse_adoc_table(&lines, i, col_hint.take());
            render_table(&rows, &mut out);
            i = next_i;
            continue;
        }

        if trimmed == "'''" || (trimmed.chars().all(|c| c == '-') && trimmed.len() == 3) {
            out.push_str("---\n\n");
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("image::")
            && let Some(bpos) = rest.find('[') {
                let path = &rest[..bpos];
                let alt = rest[bpos + 1..].trim_end_matches(']');
                out.push_str(&format!("![{alt}]({path})\n\n"));
                i += 1;
                continue;
            }

        let adm_para = ADMONITIONS
            .iter()
            .find_map(|adm| trimmed.strip_prefix(&format!("{adm}: ")).map(|rest| (*adm, rest)));
        if let Some((adm, rest)) = adm_para {
            let label = capitalize(&adm.to_ascii_lowercase());
            out.push_str(&format!("> **{label}:** {}\n\n", inline_adoc_to_md(rest)));
            i += 1;
            continue;
        }

        if trimmed.starts_with('*') {
            let depth = trimmed.chars().take_while(|&c| c == '*').count();
            if trimmed.as_bytes().get(depth) == Some(&b' ') {
                let rest = trimmed[depth + 1..].trim();
                out.push_str(&"  ".repeat(depth.saturating_sub(1)));
                out.push_str("- ");
                out.push_str(&inline_adoc_to_md(rest));
                out.push('\n');
                i += 1;
                continue;
            }
        }
        if trimmed.starts_with('.') {
            let depth = trimmed.chars().take_while(|&c| c == '.').count();
            if depth < 4 && trimmed.as_bytes().get(depth) == Some(&b' ') {
                let rest = trimmed[depth + 1..].trim();
                out.push_str(&"  ".repeat(depth.saturating_sub(1)));
                out.push_str("1. ");
                out.push_str(&inline_adoc_to_md(rest));
                out.push('\n');
                i += 1;
                continue;
            }
        }

        let mut para = vec![trimmed.to_string()];
        i += 1;
        while i < lines.len() && !lines[i].trim().is_empty() && !is_special_adoc_line(lines[i]) {
            para.push(lines[i].trim().to_string());
            i += 1;
        }
        out.push_str(&inline_adoc_to_md(&para.join(" ")));
        out.push_str("\n\n");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heading() {
        assert_eq!(convert_adoc("= Title\n\nBody.\n"), "# Title\n\nBody.\n\n");
        assert_eq!(convert_adoc("== Sub\n"), "## Sub\n\n");
    }

    #[test]
    fn test_inline_markup() {
        let out = convert_adoc("*bold* and _italic_ and `code`\n");
        assert!(out.contains("**bold**"));
        assert!(out.contains("*italic*"));
        assert!(out.contains("`code`"));
    }

    #[test]
    fn test_source_block() {
        let input = "[source,rust]\n----\nfn main() {}\n----\n";
        assert_eq!(convert_adoc(input), "```rust\nfn main() {}\n```\n\n");
    }

    #[test]
    fn test_plain_code_block() {
        let input = "----\nplain text\n----\n";
        assert_eq!(convert_adoc(input), "```\nplain text\n```\n\n");
    }

    #[test]
    fn test_lists() {
        assert_eq!(convert_adoc("* one\n* two\n"), "- one\n- two\n");
        assert_eq!(convert_adoc(". one\n. two\n"), "1. one\n1. two\n");
    }

    #[test]
    fn test_admonition_paragraph() {
        let out = convert_adoc("NOTE: Something important.\n");
        assert_eq!(out, "> **Note:** Something important.\n\n");
    }

    #[test]
    fn test_admonition_block() {
        let input = "[NOTE]\n====\nBe careful.\n====\n";
        let out = convert_adoc(input);
        assert!(out.contains("> **Note:**"));
        assert!(out.contains("> Be careful."));
    }

    #[test]
    fn test_table() {
        let input = "[cols=\"1,1\"]\n|===\n|A |B\n\n|1 |2\n|===\n";
        let out = convert_adoc(input);
        assert!(out.contains("| A | B |"));
        assert!(out.contains("| --- | --- |"));
        assert!(out.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_link() {
        assert_eq!(
            convert_adoc("See https://example.com[Example] for more.\n"),
            "See [Example](https://example.com) for more.\n\n"
        );
    }

    #[test]
    fn test_image() {
        assert_eq!(convert_adoc("image::foo.png[Alt text]\n"), "![Alt text](foo.png)\n\n");
    }

    #[test]
    fn test_converter_trait() {
        let converter = AsciidocConverter;
        let mut buf = Vec::new();
        converter.convert(b"= Title\n\nHello.\n", &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "# Title\n\nHello.\n\n");
        assert_eq!(converter.format_name(), "asciidoc");
        assert_eq!(converter.output_extension(), "md");
    }
}
