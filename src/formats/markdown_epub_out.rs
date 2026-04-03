use std::io::Write;

use epub_builder::{EpubBuilder, EpubContent, EpubVersion, ReferenceType, ZipLibrary};
use mq_markdown::{Markdown, Node};

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct MarkdownEpubConverter;

impl Converter for MarkdownEpubConverter {
    fn format_name(&self) -> &'static str {
        "markdown-epub"
    }

    fn output_extension(&self) -> &'static str {
        "epub"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let markdown = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "markdown-epub",
            message: format!("Input is not valid UTF-8: {e}"),
        })?;

        let parsed = markdown.parse::<Markdown>().map_err(|e| Error::Conversion {
            format: "markdown-epub",
            message: e.to_string(),
        })?;

        build_epub(&parsed, writer).map_err(|e| Error::Conversion {
            format: "markdown-epub",
            message: e.to_string(),
        })?;

        Ok(())
    }
}

fn extract_heading_text(nodes: &[Node]) -> Option<String> {
    for node in nodes {
        if let Node::Heading(h) = node {
            if h.depth == 1 {
                return Some(extract_text(&h.values));
            }
        }
    }
    None
}

fn extract_text(nodes: &[Node]) -> String {
    let mut out = String::new();
    for node in nodes {
        match node {
            Node::Text(t) => out.push_str(&t.value),
            Node::Strong(s) => out.push_str(&extract_text(&s.values)),
            Node::Emphasis(e) => out.push_str(&extract_text(&e.values)),
            Node::CodeInline(c) => out.push_str(&c.value),
            Node::Link(l) => out.push_str(&extract_text(&l.values)),
            Node::Delete(d) => out.push_str(&extract_text(&d.values)),
            Node::Break(_) => out.push(' '),
            _ => {}
        }
    }
    out
}

/// Split the document into chapters at each top-level H1 boundary.
/// Returns a Vec of (title, markdown_string) pairs.
fn split_into_chapters(nodes: &[Node]) -> Vec<(String, Vec<&Node>)> {
    let mut chapters: Vec<(String, Vec<&Node>)> = Vec::new();
    let mut current_title = "Document".to_string();
    let mut current_nodes: Vec<&Node> = Vec::new();

    for node in nodes {
        if let Node::Heading(h) = node {
            if h.depth == 1 {
                if !current_nodes.is_empty() || !chapters.is_empty() {
                    chapters.push((current_title.clone(), std::mem::take(&mut current_nodes)));
                }
                current_title = extract_text(&h.values);
            }
        }
        current_nodes.push(node);
    }

    if !current_nodes.is_empty() {
        chapters.push((current_title, current_nodes));
    }

    if chapters.is_empty() {
        chapters.push(("Document".to_string(), nodes.iter().collect()));
    }

    chapters
}

fn nodes_to_html(nodes: &[&Node]) -> String {
    let mut md = String::new();
    for node in nodes {
        match node {
            Node::Heading(h) => {
                let prefix = "#".repeat(h.depth as usize);
                let text = extract_text(&h.values);
                md.push_str(&format!("{prefix} {text}\n\n"));
            }
            Node::Code(c) => {
                if let Some(lang) = &c.lang {
                    md.push_str(&format!("```{lang}\n{}\n```\n\n", c.value));
                } else {
                    md.push_str(&format!("```\n{}\n```\n\n", c.value));
                }
            }
            Node::List(l) => {
                let text = extract_text(&l.values);
                if l.ordered {
                    md.push_str(&format!("1. {text}\n"));
                } else {
                    md.push_str(&format!("- {text}\n"));
                }
            }
            Node::Blockquote(bq) => {
                let text = extract_text(&bq.values);
                md.push_str(&format!("> {text}\n\n"));
            }
            Node::HorizontalRule(_) => {
                md.push_str("---\n\n");
            }
            Node::Text(t) => {
                md.push_str(&t.value);
                md.push('\n');
            }
            Node::Strong(s) => {
                let text = extract_text(&s.values);
                md.push_str(&format!("**{text}**"));
            }
            Node::Emphasis(e) => {
                let text = extract_text(&e.values);
                md.push_str(&format!("*{text}*"));
            }
            Node::Link(l) => {
                let text = extract_text(&l.values);
                let url = l.url.as_str();
                md.push_str(&format!("[{text}]({url})"));
            }
            _ => {}
        }
    }

    // Convert the accumulated markdown to HTML using mq-markdown
    let parsed = md.parse::<Markdown>();
    match parsed {
        Ok(doc) => {
            let html = doc.to_html();
            wrap_xhtml(&html)
        }
        Err(_) => wrap_xhtml(&format!("<pre>{}</pre>", html_escape(&md))),
    }
}

fn wrap_xhtml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><meta charset="UTF-8"/></head>
<body>
{body}
</body>
</html>"#
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn build_epub(parsed: &Markdown, writer: &mut dyn Write) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let title = extract_heading_text(&parsed.nodes)
        .unwrap_or_else(|| "Untitled".to_string());

    let chapters = split_into_chapters(&parsed.nodes);

    let zip = ZipLibrary::new()?;
    let mut builder = EpubBuilder::new(zip)?;
    builder
        .epub_version(EpubVersion::V30)
        .metadata("title", &title)?;

    for (i, (chapter_title, chapter_nodes)) in chapters.iter().enumerate() {
        let filename = format!("chapter_{i}.xhtml");
        let html = nodes_to_html(chapter_nodes);

        let content = if i == 0 {
            EpubContent::new(&filename, html.as_bytes())
                .title(chapter_title)
                .reftype(ReferenceType::Text)
        } else {
            EpubContent::new(&filename, html.as_bytes()).title(chapter_title)
        };

        builder.add_content(content)?;
    }

    builder.generate(writer)?;

    Ok(())
}
