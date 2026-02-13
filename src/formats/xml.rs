use std::io::Write;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct XmlConverter;

impl Converter for XmlConverter {
    fn format_name(&self) -> &'static str {
        "xml"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let text = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "xml",
            message: e.to_string(),
        })?;

        let root = parse_xml(text)?;
        write_element(writer, &root, 1)?;

        Ok(())
    }
}

struct XmlElement {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<XmlNode>,
}

enum XmlNode {
    Element(XmlElement),
    Text(String),
}

fn parse_xml(text: &str) -> Result<XmlElement> {
    let mut reader = Reader::from_str(text);
    let mut stack: Vec<XmlElement> = Vec::new();
    let mut root: Option<XmlElement> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = local_name(e.name().as_ref());
                let attributes: Vec<(String, String)> = e
                    .attributes()
                    .flatten()
                    .map(|a| {
                        (
                            String::from_utf8_lossy(a.key.as_ref()).to_string(),
                            String::from_utf8_lossy(&a.value).to_string(),
                        )
                    })
                    .collect();
                stack.push(XmlElement {
                    name,
                    attributes,
                    children: Vec::new(),
                });
            }
            Ok(Event::Empty(e)) => {
                let name = local_name(e.name().as_ref());
                let attributes: Vec<(String, String)> = e
                    .attributes()
                    .flatten()
                    .map(|a| {
                        (
                            String::from_utf8_lossy(a.key.as_ref()).to_string(),
                            String::from_utf8_lossy(&a.value).to_string(),
                        )
                    })
                    .collect();
                let elem = XmlElement {
                    name,
                    attributes,
                    children: Vec::new(),
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(XmlNode::Element(elem));
                } else {
                    root = Some(elem);
                }
            }
            Ok(Event::Text(e)) => {
                let text = e.decode().unwrap_or_default().trim().to_string();
                if !text.is_empty()
                    && let Some(parent) = stack.last_mut() {
                        parent.children.push(XmlNode::Text(text));
                    }
            }
            Ok(Event::CData(e)) => {
                let text = String::from_utf8_lossy(e.as_ref()).trim().to_string();
                if !text.is_empty()
                    && let Some(parent) = stack.last_mut() {
                        parent.children.push(XmlNode::Text(text));
                    }
            }
            Ok(Event::End(_)) => {
                if let Some(elem) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(XmlNode::Element(elem));
                    } else {
                        root = Some(elem);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::Conversion {
                    format: "xml",
                    message: format!("Invalid XML: {e}"),
                });
            }
            _ => {}
        }
    }

    root.ok_or_else(|| Error::Conversion {
        format: "xml",
        message: "Empty XML document".into(),
    })
}

fn write_element(writer: &mut dyn Write, elem: &XmlElement, depth: usize) -> Result<()> {
    let level = depth.min(6);
    let hashes = "#".repeat(level);
    writeln!(writer, "{hashes} {}", elem.name)?;
    writeln!(writer)?;

    // Write attributes as a table
    if !elem.attributes.is_empty() {
        writeln!(writer, "| Attribute | Value |")?;
        writeln!(writer, "|---|---|")?;
        for (key, val) in &elem.attributes {
            writeln!(writer, "| {} | {} |", escape_pipe(key), escape_pipe(val))?;
        }
        writeln!(writer)?;
    }

    // Separate text nodes and element children
    let mut text_parts: Vec<&str> = Vec::new();
    let mut child_elements: Vec<&XmlElement> = Vec::new();

    for child in &elem.children {
        match child {
            XmlNode::Text(t) => text_parts.push(t),
            XmlNode::Element(e) => child_elements.push(e),
        }
    }

    // Write text content
    if !text_parts.is_empty() {
        for text in &text_parts {
            writeln!(writer, "{text}")?;
        }
        writeln!(writer)?;
    }

    // Try to group repeated same-name child elements into a table
    if !child_elements.is_empty() {
        let mut i = 0;
        while i < child_elements.len() {
            // Find a run of same-named elements
            let name = &child_elements[i].name;
            let mut end = i + 1;
            while end < child_elements.len() && child_elements[end].name == *name {
                end += 1;
            }

            if end - i > 1 && can_table_elements(&child_elements[i..end]) {
                write_elements_as_table(writer, &child_elements[i..end], depth)?;
                i = end;
            } else {
                // Write each element as a subsection
                while i < end {
                    write_element(writer, child_elements[i], depth + 1)?;
                    i += 1;
                }
            }
        }
    }

    Ok(())
}

/// Check if a group of same-named elements can be represented as a table.
/// They must all have only attributes and/or a single text child, no nested elements.
fn can_table_elements(elements: &[&XmlElement]) -> bool {
    elements.iter().all(|e| {
        let has_child_elements = e
            .children
            .iter()
            .any(|c| matches!(c, XmlNode::Element(_)));
        !has_child_elements
    })
}

fn write_elements_as_table(
    writer: &mut dyn Write,
    elements: &[&XmlElement],
    depth: usize,
) -> Result<()> {
    let level = (depth + 1).min(6);
    let hashes = "#".repeat(level);
    writeln!(writer, "{hashes} {}", elements[0].name)?;
    writeln!(writer)?;

    // Collect all attribute names + "text" column if any have text
    let mut headers: Vec<String> = Vec::new();
    let mut has_text = false;

    for elem in elements {
        for (key, _) in &elem.attributes {
            if !headers.contains(key) {
                headers.push(key.clone());
            }
        }
        let text: String = elem
            .children
            .iter()
            .filter_map(|c| match c {
                XmlNode::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        if !text.is_empty() {
            has_text = true;
        }
    }

    if has_text {
        headers.push("text".to_string());
    }

    if headers.is_empty() {
        return Ok(());
    }

    // Header row
    write!(writer, "|")?;
    for h in &headers {
        write!(writer, " {} |", escape_pipe(h))?;
    }
    writeln!(writer)?;

    // Separator
    write!(writer, "|")?;
    for _ in &headers {
        write!(writer, "---|")?;
    }
    writeln!(writer)?;

    // Data rows
    for elem in elements {
        write!(writer, "|")?;
        for h in &headers {
            let val = if h == "text" {
                elem.children
                    .iter()
                    .filter_map(|c| match c {
                        XmlNode::Text(t) => Some(t.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                elem.attributes
                    .iter()
                    .find(|(k, _)| k == h)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            write!(writer, " {} |", escape_pipe(&val))?;
        }
        writeln!(writer)?;
    }
    writeln!(writer)?;

    Ok(())
}

fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|")
}

fn local_name(name: &[u8]) -> String {
    let s = std::str::from_utf8(name).unwrap_or("");
    if let Some(pos) = s.rfind(':') {
        s[pos + 1..].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::Converter;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    fn convert(input: &str) -> String {
        let converter = XmlConverter;
        let mut output = Vec::new();
        converter.convert(input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[rstest]
    #[case::simple_element(
        "<root>hello</root>",
        "# root\n\nhello\n\n"
    )]
    #[case::element_with_attributes(
        r#"<item id="1" name="test"/>"#,
        "# item\n\n| Attribute | Value |\n|---|---|\n| id | 1 |\n| name | test |\n\n"
    )]
    #[case::nested_elements(
        "<root><child>text</child></root>",
        "# root\n\n## child\n\ntext\n\n"
    )]
    #[case::attributes_and_text(
        r#"<book lang="en">Rust Guide</book>"#,
        "# book\n\n| Attribute | Value |\n|---|---|\n| lang | en |\n\nRust Guide\n\n"
    )]
    fn test_basic(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    #[case::repeated_elements_as_table(
        r#"<list><item id="1">A</item><item id="2">B</item></list>"#,
        "# list\n\n## item\n\n| id | text |\n|---|---|\n| 1 | A |\n| 2 | B |\n\n"
    )]
    #[case::repeated_empty_elements_as_table(
        r#"<data><row x="1" y="2"/><row x="3" y="4"/></data>"#,
        "# data\n\n## row\n\n| x | y |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n\n"
    )]
    fn test_table_grouping(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    fn test_deep_nesting() {
        let output = convert("<a><b><c><d>deep</d></c></b></a>");
        assert!(output.contains("# a"));
        assert!(output.contains("## b"));
        assert!(output.contains("### c"));
        assert!(output.contains("#### d"));
        assert!(output.contains("deep"));
    }

    #[rstest]
    fn test_pipe_escape() {
        let output = convert(r#"<item val="a|b"/>"#);
        assert!(output.contains("a\\|b"));
    }

    #[rstest]
    fn test_empty_xml_error() {
        let converter = XmlConverter;
        let mut output = Vec::new();
        let result = converter.convert(b"", &mut output);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_mixed_children() {
        let output = convert(r#"<root><a>text</a><b x="1"/><b x="2"/></root>"#);
        assert!(output.contains("## a"));
        assert!(output.contains("text"));
        assert!(output.contains("## b"));
        assert!(output.contains("| x |"));
    }
}
