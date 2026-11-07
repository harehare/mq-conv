//! Jupyter notebooks (`.ipynb`) → Markdown.
//!
//! Markdown cells are kept as they are, code cells become fenced code in the
//! notebook's language, and outputs follow their cell as plain-text blocks.
//! Rich outputs are reduced to what reads well as Markdown: Markdown first,
//! then HTML tables, then plain text. Image outputs are saved when an image
//! directory was requested.

use std::io::Write;

use serde_json::Value;

use crate::converter::{ConvertOptions, Converter};
use crate::document::{Block, Document, Inline};
use crate::error::{Error, Result};
use crate::formats::media::MediaWriter;

pub struct IpynbConverter;

impl Converter for IpynbConverter {
    fn format_name(&self) -> &'static str {
        "ipynb"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        self.convert_with(input, writer, &ConvertOptions::default())
    }

    fn convert_with(
        &self,
        input: &[u8],
        writer: &mut dyn Write,
        options: &ConvertOptions,
    ) -> Result<()> {
        let notebook: Value = serde_json::from_slice(input).map_err(|e| Error::Conversion {
            format: "ipynb",
            message: format!("not a notebook: {e}"),
        })?;
        let cells = cells_of(&notebook).ok_or_else(|| Error::Conversion {
            format: "ipynb",
            message: "the notebook has no cells".to_string(),
        })?;
        let language = language_of(&notebook);

        let mut media = MediaWriter::from_options(options);
        let mut doc = Document::new();
        for (index, cell) in cells.iter().enumerate() {
            convert_cell(cell, index + 1, language.as_deref(), &mut media, &mut doc);
        }
        doc.write_markdown(writer)
    }
}

/// The cells of a notebook: `cells` (nbformat 4) or the cells of every
/// worksheet (nbformat 3).
fn cells_of(notebook: &Value) -> Option<Vec<&Value>> {
    if let Some(cells) = notebook.get("cells").and_then(Value::as_array) {
        return Some(cells.iter().collect());
    }
    let sheets = notebook.get("worksheets")?.as_array()?;
    Some(
        sheets
            .iter()
            .filter_map(|s| s.get("cells").and_then(Value::as_array))
            .flatten()
            .collect(),
    )
}

fn language_of(notebook: &Value) -> Option<String> {
    let meta = notebook.get("metadata")?;
    [
        meta.pointer("/language_info/name"),
        meta.pointer("/kernelspec/language"),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_str)
    .map(str::to_lowercase)
}

/// A string, or the lines of a list of strings, as one text.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(lines) => lines.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    }
}

fn convert_cell(
    cell: &Value,
    number: usize,
    language: Option<&str>,
    media: &mut Option<MediaWriter>,
    doc: &mut Document,
) {
    let kind = cell.get("cell_type").and_then(Value::as_str).unwrap_or("");
    // nbformat 3 called the source of a code cell `input`.
    let source = cell
        .get("source")
        .or_else(|| cell.get("input"))
        .map(text_of)
        .unwrap_or_default();
    match kind {
        "markdown" => {
            if !source.trim().is_empty() {
                doc.push(Block::Markdown(source));
            }
        }
        "heading" => {
            let level = cell.get("level").and_then(Value::as_u64).unwrap_or(1);
            doc.push(Block::heading(level.clamp(1, 6) as u8, source.trim()));
        }
        "raw" => {
            if !source.trim().is_empty() {
                doc.push(Block::Code {
                    lang: None,
                    text: source.trim_end().to_string(),
                });
            }
        }
        "code" => {
            if !source.trim().is_empty() {
                doc.push(Block::Code {
                    lang: language.map(str::to_string),
                    text: source.trim_end().to_string(),
                });
            }
            let outputs = cell.get("outputs").and_then(Value::as_array);
            for (n, output) in outputs.into_iter().flatten().enumerate() {
                convert_output(output, number, n + 1, media, doc);
            }
        }
        _ => {}
    }
}

fn convert_output(
    output: &Value,
    cell: usize,
    n: usize,
    media: &mut Option<MediaWriter>,
    doc: &mut Document,
) {
    match output.get("output_type").and_then(Value::as_str) {
        Some("stream") => {
            let text = output.get("text").map(text_of).unwrap_or_default();
            push_output_text(doc, &text);
        }
        Some("error") => {
            let traceback = output.get("traceback").map(text_of).unwrap_or_default();
            if traceback.trim().is_empty() {
                let name = output.get("ename").and_then(Value::as_str).unwrap_or("");
                let value = output.get("evalue").and_then(Value::as_str).unwrap_or("");
                push_output_text(doc, &format!("{name}: {value}"));
            } else {
                // A traceback is a list of lines, each ending without a newline.
                let lines: Vec<String> = output
                    .get("traceback")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                push_output_text(doc, &lines.join("\n"));
            }
        }
        Some("execute_result" | "display_data" | "pyout") => {
            let data = output.get("data").unwrap_or(output);
            convert_data(data, cell, n, media, doc);
        }
        _ => {}
    }
}

/// One rich output: the first representation that reads well as Markdown.
fn convert_data(
    data: &Value,
    cell: usize,
    n: usize,
    media: &mut Option<MediaWriter>,
    doc: &mut Document,
) {
    // Images have no text form; they are kept only when they can be saved.
    let mut image = None;
    for (mime, ext) in [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/gif", "gif"),
    ] {
        if let Some(encoded) = data.get(mime).map(text_of) {
            image = decode_base64(&encoded).map(|bytes| (bytes, ext));
            break;
        }
    }
    if image.is_none()
        && let Some(svg) = data.get("image/svg+xml").map(text_of)
    {
        image = Some((svg.into_bytes(), "svg"));
    }
    if let (Some((bytes, ext)), Some(media)) = (&image, media.as_mut())
        && let Some(url) = media.save(&format!("cell{cell}-output{n}.{ext}"), bytes)
    {
        doc.push(Block::Paragraph(vec![Inline::Image {
            url,
            alt: "output".to_string(),
        }]));
        return;
    }

    if let Some(markdown) = data.get("text/markdown").map(text_of)
        && !markdown.trim().is_empty()
    {
        doc.push(Block::Markdown(markdown));
        return;
    }
    // DataFrames and the like come as an HTML table next to a text rendering.
    if let Some(html) = data.get("text/html").map(text_of)
        && html.contains("<table")
    {
        let markdown = html_to_markdown(&strip_styles(&html));
        if markdown.trim().contains('|') {
            doc.push(Block::Markdown(markdown));
            return;
        }
    }
    if let Some(text) = data.get("text/plain").or_else(|| data.get("text")) {
        push_output_text(doc, &text_of(text));
    }
}

fn push_output_text(doc: &mut Document, text: &str) {
    let text = strip_ansi(text);
    if text.trim().is_empty() {
        return;
    }
    doc.push(Block::Code {
        lang: Some("text".to_string()),
        text: text.trim_end().to_string(),
    });
}

/// Removes ANSI escape sequences, which terminals use for colour in
/// tracebacks and progress output.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'[') {
            chars.next();
            // Parameters, then one final byte in `@`..`~`.
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

/// `<style>` blocks that pandas puts in front of its tables.
fn strip_styles(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<style") {
        out.push_str(&rest[..start]);
        match rest[start..].find("</style>") {
            Some(end) => rest = &rest[start + end + "</style>".len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn html_to_markdown(html: &str) -> String {
    mq_markdown::convert_html_to_markdown(
        html,
        mq_markdown::ConversionOptions {
            extract_scripts_as_code_blocks: false,
            generate_front_matter: false,
            use_title_as_h1: false,
            base_url: None,
        },
    )
    .unwrap_or_default()
}

/// Standard base64, ignoring whitespace and padding. `None` on any other byte.
fn decode_base64(encoded: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(encoded.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in encoded.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn convert_json(notebook: &Value) -> String {
        let mut out = Vec::new();
        IpynbConverter
            .convert(notebook.to_string().as_bytes(), &mut out)
            .unwrap();
        String::from_utf8(out).unwrap()
    }

    fn notebook(cells: Value) -> Value {
        json!({
            "nbformat": 4,
            "metadata": {"language_info": {"name": "Python"}},
            "cells": cells,
        })
    }

    #[test]
    fn markdown_and_code_cells_with_stream_output() {
        let nb = notebook(json!([
            {"cell_type": "markdown", "source": ["# Title\n", "Some *text*."]},
            {"cell_type": "code", "source": "print('hi')\n", "outputs": [
                {"output_type": "stream", "name": "stdout", "text": ["hi\n"]}
            ]},
        ]));
        assert_eq!(
            convert_json(&nb),
            "# Title\nSome *text*.\n\n```python\nprint('hi')\n```\n\n```text\nhi\n```\n"
        );
    }

    #[test]
    fn execute_result_prefers_markdown_then_html_table_then_text() {
        let nb = notebook(json!([
            {"cell_type": "code", "source": "df", "outputs": [{
                "output_type": "execute_result",
                "data": {
                    "text/plain": "   a\n0  1",
                    "text/html": "<style scoped>td {color: red}</style><table><thead><tr><th>a</th></tr></thead><tbody><tr><td>1</td></tr></tbody></table>"
                }
            }]},
            {"cell_type": "code", "source": "1+1", "outputs": [{
                "output_type": "execute_result",
                "data": {"text/plain": "2"}
            }]},
        ]));
        let out = convert_json(&nb);
        assert!(out.contains("| a "), "table missing in:\n{out}");
        assert!(!out.contains("color: red"), "style leaked into:\n{out}");
        assert!(out.contains("```text\n2\n```"), "{out}");
    }

    #[test]
    fn errors_lose_their_ansi_colours() {
        let nb = notebook(json!([
            {"cell_type": "code", "source": "1/0", "outputs": [{
                "output_type": "error", "ename": "ZeroDivisionError", "evalue": "division by zero",
                "traceback": ["\u{1b}[0;31mZeroDivisionError\u{1b}[0m: division by zero"]
            }]},
        ]));
        let out = convert_json(&nb);
        assert!(
            out.contains("```text\nZeroDivisionError: division by zero\n```"),
            "{out}"
        );
        assert!(!out.contains('\u{1b}'), "{out:?}");
    }

    #[test]
    fn empty_cells_are_skipped_and_raw_cells_kept_verbatim() {
        let nb = notebook(json!([
            {"cell_type": "code", "source": "", "outputs": []},
            {"cell_type": "markdown", "source": ["  "]},
            {"cell_type": "raw", "source": "key: value"},
        ]));
        assert_eq!(convert_json(&nb), "```\nkey: value\n```\n");
    }

    #[test]
    fn image_outputs_are_saved_only_when_asked() {
        let nb = notebook(json!([
            {"cell_type": "code", "source": "plot()", "outputs": [{
                "output_type": "display_data",
                "data": {"image/png": "iVBORw0K\n", "text/plain": "<Figure>"}
            }]},
        ]));
        // Without a directory the figure's text stand-in is all there is.
        assert!(convert_json(&nb).contains("<Figure>"));

        let dir = std::env::temp_dir().join(format!("mq-conv-ipynb-{}", std::process::id()));
        let options = ConvertOptions {
            image_dir: Some(dir.clone()),
            ..Default::default()
        };
        let mut out = Vec::new();
        IpynbConverter
            .convert_with(nb.to_string().as_bytes(), &mut out, &options)
            .unwrap();
        let md = String::from_utf8(out).unwrap();
        assert!(md.contains("![output]("), "{md}");
        assert!(!md.contains("<Figure>"), "{md}");
        assert_eq!(
            std::fs::read(dir.join("cell1-output1.png")).unwrap(),
            [0x89, b'P', b'N', b'G', 0x0d, 0x0a]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn nbformat_3_worksheets_are_read() {
        let nb = json!({"nbformat": 3, "worksheets": [{"cells": [
            {"cell_type": "heading", "level": 2, "source": ["Intro"]},
            {"cell_type": "code", "language": "python", "input": ["x = 1"], "outputs": []},
        ]}]});
        assert_eq!(convert_json(&nb), "## Intro\n\n```\nx = 1\n```\n");
    }

    #[test]
    fn rejects_input_that_is_not_a_notebook() {
        let mut out = Vec::new();
        assert!(IpynbConverter.convert(b"not json", &mut out).is_err());
        assert!(IpynbConverter.convert(b"{}", &mut out).is_err());
    }

    #[test]
    fn base64_decoding() {
        assert_eq!(decode_base64("aGk=").unwrap(), b"hi");
        assert_eq!(decode_base64("aG\nk").unwrap(), b"hi");
        assert!(decode_base64("a*b").is_none());
    }
}
