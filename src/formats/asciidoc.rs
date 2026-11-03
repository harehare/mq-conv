use std::io::Write;

use crate::converter::Converter;
use crate::document::{Block, Builder, Document, Inline};
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

        parse_adoc(text).write_markdown(writer)
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

/// The URL and label of `link:url[text]` or `http(s)://url[text]` starting at `i`,
/// and the index after it.
fn adoc_link(chars: &[char], i: usize) -> Option<(String, String, usize)> {
    let starts = |lit: &str| {
        lit.chars()
            .enumerate()
            .all(|(k, c)| chars.get(i + k) == Some(&c))
    };
    let url_start = i + if starts("link:") { 5 } else { 0 };
    if url_start == i && !starts("http://") && !starts("https://") {
        return None;
    }
    let rel = chars[url_start..]
        .iter()
        .position(|&c| c == '[' || c.is_whitespace())?;
    if chars[url_start + rel] != '[' {
        return None;
    }
    let bracket = url_start + rel;
    let close = bracket + 1 + chars[bracket + 1..].iter().position(|&c| c == ']')?;
    let url: String = chars[url_start..bracket].iter().collect();
    let label: String = chars[bracket + 1..close].iter().collect();
    let label = if label.is_empty() { url.clone() } else { label };
    Some((url, label, close + 1))
}

/// Inline markup: `*bold*`, `_italic_`, `` `code` `` and links.
fn inline_adoc(s: &str) -> Vec<Inline> {
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
        if let Some((url, label, next)) = adoc_link(&chars, i) {
            flush(&mut text, &mut out);
            out.push(Inline::Link {
                url,
                content: vec![Inline::Text(label)],
            });
            i = next;
            continue;
        }
        let c = chars[i];
        if matches!(c, '*' | '_' | '`') {
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
                    out.push(match c {
                        '*' => Inline::Strong(inline_adoc(&content)),
                        '_' => Inline::Emphasis(inline_adoc(&content)),
                        _ => Inline::Code(content),
                    });
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

/// Paragraphs of a quoted block: lines are joined, blank lines separate them.
fn quoted_paragraphs(block: &[String]) -> Vec<Block> {
    block
        .split(|l| l.trim().is_empty())
        .filter(|p| !p.is_empty())
        .map(|p| {
            let joined: Vec<&str> = p.iter().map(|l| l.trim()).collect();
            Block::Paragraph(inline_adoc(&joined.join(" ")))
        })
        .collect()
}

fn label(adm: &str) -> Inline {
    Inline::Strong(vec![Inline::Text(format!(
        "{}:",
        capitalize(&adm.to_ascii_lowercase())
    ))])
}

fn code(lang: Option<String>, lines: Vec<String>) -> Block {
    Block::Code {
        lang,
        text: lines.join("\n"),
    }
}

fn parse_adoc_table(
    lines: &[&str],
    start: usize,
    col_hint: Option<usize>,
) -> (Vec<Vec<Vec<Inline>>>, usize) {
    let mut i = start + 1;
    let mut cells: Vec<Vec<Inline>> = Vec::new();
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
                cells.push(inline_adoc(p));
            }
        }
        i += 1;
    }
    let col_count = col_hint
        .or(first_row_break)
        .unwrap_or_else(|| cells.len().max(1))
        .max(1);
    let rows = cells.chunks(col_count).map(|c| c.to_vec()).collect();
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

fn parse_adoc(text: &str) -> Document {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Builder::default();
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
            && let Some(colon_pos) = rest.find(':')
        {
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
                let (block, next_i) = collect_delim_block(&lines, i + 2, '=');
                let mut quote = vec![Block::Paragraph(vec![label(&upper)])];
                quote.extend(quoted_paragraphs(&block));
                out.push(Block::Quote(quote));
                i = next_i;
                continue;
            }

            if upper.starts_with("QUOTE")
                && i + 1 < lines.len()
                && is_delim(lines[i + 1].trim(), '_', 4)
            {
                let (block, next_i) = collect_delim_block(&lines, i + 2, '_');
                out.push(Block::Quote(quoted_paragraphs(&block)));
                i = next_i;
                continue;
            }

            if let Some(rest) = inner.strip_prefix("source") {
                let lang = rest.trim_start_matches(',').trim();
                if i + 1 < lines.len() && is_delim(lines[i + 1].trim(), '-', 4) {
                    let (block, next_i) = collect_delim_block(&lines, i + 2, '-');
                    out.push(code((!lang.is_empty()).then(|| lang.to_string()), block));
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
                out.push(Block::Heading {
                    level: eq as u8,
                    content: inline_adoc(trimmed[eq + 1..].trim()),
                });
                i += 1;
                continue;
            }
        }

        if is_delim(trimmed, '-', 4) {
            let (block, next_i) = collect_delim_block(&lines, i + 1, '-');
            out.push(code(None, block));
            i = next_i;
            continue;
        }
        if is_delim(trimmed, '.', 4) {
            let (block, next_i) = collect_delim_block(&lines, i + 1, '.');
            out.push(code(None, block));
            i = next_i;
            continue;
        }
        if is_delim(trimmed, '_', 4) {
            let (block, next_i) = collect_delim_block(&lines, i + 1, '_');
            out.push(Block::Quote(quoted_paragraphs(&block)));
            i = next_i;
            continue;
        }

        if trimmed.starts_with("|===") {
            let (rows, next_i) = parse_adoc_table(&lines, i, col_hint.take());
            out.push(Block::Table(rows));
            i = next_i;
            continue;
        }

        if trimmed == "'''" || (trimmed.chars().all(|c| c == '-') && trimmed.len() == 3) {
            out.push(Block::Rule);
            i += 1;
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("image::")
            && let Some(bpos) = rest.find('[')
        {
            out.push(Block::Paragraph(vec![Inline::Image {
                url: rest[..bpos].to_string(),
                alt: rest[bpos + 1..].trim_end_matches(']').to_string(),
            }]));
            i += 1;
            continue;
        }

        let adm_para = ADMONITIONS.iter().find_map(|adm| {
            trimmed
                .strip_prefix(&format!("{adm}: "))
                .map(|rest| (*adm, rest))
        });
        if let Some((adm, rest)) = adm_para {
            let mut content = vec![label(adm), Inline::Text(" ".to_string())];
            content.extend(inline_adoc(rest));
            out.push(Block::Quote(vec![Block::Paragraph(content)]));
            i += 1;
            continue;
        }

        if trimmed.starts_with('*') {
            let depth = trimmed.chars().take_while(|&c| c == '*').count();
            if trimmed.as_bytes().get(depth) == Some(&b' ') {
                out.item(depth - 1, false, inline_adoc(trimmed[depth + 1..].trim()));
                i += 1;
                continue;
            }
        }
        if trimmed.starts_with('.') {
            let depth = trimmed.chars().take_while(|&c| c == '.').count();
            if depth < 4 && trimmed.as_bytes().get(depth) == Some(&b' ') {
                out.item(depth - 1, true, inline_adoc(trimmed[depth + 1..].trim()));
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
        out.push(Block::Paragraph(inline_adoc(&para.join(" "))));
    }

    out.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert_adoc(text: &str) -> String {
        let mut out = Vec::new();
        parse_adoc(text).write_markdown(&mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn test_heading() {
        assert_eq!(convert_adoc("= Title\n\nBody.\n"), "# Title\n\nBody.\n");
        assert_eq!(convert_adoc("== Sub\n"), "## Sub\n");
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
        assert_eq!(convert_adoc(input), "```rust\nfn main() {}\n```\n");
    }

    #[test]
    fn test_plain_code_block() {
        let input = "----\nplain text\n----\n";
        assert_eq!(convert_adoc(input), "```\nplain text\n```\n");
    }

    #[test]
    fn test_lists() {
        assert_eq!(convert_adoc("* one\n* two\n"), "- one\n- two\n");
        assert_eq!(convert_adoc(". one\n. two\n"), "1. one\n2. two\n");
    }

    #[test]
    fn test_admonition_paragraph() {
        let out = convert_adoc("NOTE: Something important.\n");
        assert_eq!(out, "> **Note:** Something important.\n");
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
        assert!(out.contains("| - | - |"));
        assert!(out.contains("| 1 | 2 |"));
    }

    #[test]
    fn test_link() {
        assert_eq!(
            convert_adoc("See https://example.com[Example] for more.\n"),
            "See [Example](https://example.com) for more.\n"
        );
    }

    #[test]
    fn test_image() {
        assert_eq!(
            convert_adoc("image::foo.png[Alt text]\n"),
            "![Alt text](foo.png)\n"
        );
    }

    #[test]
    fn test_nested_lists_and_following_paragraph() {
        let out = convert_adoc("* a\n** a1\n* b\n\nAfter\n");
        assert_eq!(out, "- a\n  - a1\n- b\n\nAfter\n");
    }

    #[test]
    fn test_quote_block_joins_lines_into_paragraphs() {
        let out = convert_adoc("[quote]\n____\nOne\ntwo\n\nThree\n____\n");
        assert_eq!(out, "> One two\n>\n> Three\n");
    }

    #[test]
    fn test_converter_trait() {
        let converter = AsciidocConverter;
        let mut buf = Vec::new();
        converter.convert(b"= Title\n\nHello.\n", &mut buf).unwrap();
        assert_eq!(String::from_utf8(buf).unwrap(), "# Title\n\nHello.\n");
        assert_eq!(converter.format_name(), "asciidoc");
        assert_eq!(converter.output_extension(), "md");
    }
}
