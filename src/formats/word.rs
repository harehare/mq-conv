use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::converter::{ConvertOptions, Converter};
use crate::document::{Block, Builder, Document, Inline, trim_inlines};
use crate::error::{Error, Result};
use crate::formats::media::{MediaWriter, extract_related_images, parse_relationships};
use crate::formats::xmlutil::entity_text;

pub struct WordConverter;

impl Converter for WordConverter {
    fn format_name(&self) -> &'static str {
        "word"
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
        let cursor = Cursor::new(input);
        let mut archive = zip::ZipArchive::new(cursor).map_err(|e| Error::Conversion {
            format: "word",
            message: e.to_string(),
        })?;

        let rels = read_entry(&mut archive, "word/_rels/document.xml.rels")
            .map(|xml| parse_relationships(&xml))
            .unwrap_or_default();
        let images = MediaWriter::from_options(options)
            .map(|mut media| extract_related_images(&mut archive, "word/", &rels, &mut media))
            .unwrap_or_default();
        let numbering = read_entry(&mut archive, "word/numbering.xml")
            .map(|xml| parse_numbering(&xml))
            .unwrap_or_default();
        let notes = NoteDefs {
            comments: read_entry(&mut archive, "word/comments.xml")
                .map(|xml| parse_comments(&xml))
                .unwrap_or_default(),
            footnotes: read_entry(&mut archive, "word/footnotes.xml")
                .map(|xml| parse_notes(&xml, "footnote"))
                .unwrap_or_default(),
            endnotes: read_entry(&mut archive, "word/endnotes.xml")
                .map(|xml| parse_notes(&xml, "endnote"))
                .unwrap_or_default(),
        };
        let document_xml = read_entry(&mut archive, "word/document.xml")?;

        let ctx = Context {
            rels: &rels,
            numbering: &numbering,
            images: &images,
            notes: &notes,
        };
        parse_document(&document_xml, &ctx)?.write_markdown(writer)
    }
}

/// The comments, footnotes and endnotes of a document, by their ids.
#[derive(Default)]
struct NoteDefs {
    comments: HashMap<String, Vec<Block>>,
    footnotes: HashMap<String, Vec<Block>>,
    endnotes: HashMap<String, Vec<Block>>,
}

/// What `parse_document` needs besides the XML itself.
struct Context<'a> {
    rels: &'a HashMap<String, String>,
    numbering: &'a HashMap<(String, u32), bool>,
    images: &'a HashMap<String, String>,
    notes: &'a NoteDefs,
}

/// A `w:comment`, `w:footnote` or `w:endnote` element: its attributes and the
/// XML between its tags.
struct NoteElement {
    id: String,
    author: Option<String>,
    date: Option<String>,
    kind: Option<String>,
    xml: String,
}

fn split_notes(xml: &str, element: &str) -> Vec<NoteElement> {
    let mut reader = Reader::from_str(xml);
    let mut found = Vec::new();
    let mut open: Option<(NoteElement, usize)> = None;
    loop {
        let before = reader.buffer_position() as usize;
        match reader.read_event() {
            Ok(Event::Start(e)) if local_name(e.name().as_ref()) == element => {
                if let Some(id) = attr_value(&e, "id") {
                    let note = NoteElement {
                        id,
                        author: attr_value(&e, "author"),
                        date: attr_value(&e, "date"),
                        kind: attr_value(&e, "type"),
                        xml: String::new(),
                    };
                    open = Some((note, reader.buffer_position() as usize));
                }
            }
            Ok(Event::End(e)) if local_name(e.name().as_ref()) == element => {
                if let Some((mut note, start)) = open.take() {
                    note.xml = xml.get(start..before).unwrap_or_default().to_string();
                    found.push(note);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    found
}

/// The blocks of a note body. Notes hold no further notes, and their
/// hyperlinks and images live in parts that are not read.
fn note_blocks(xml: &str) -> Vec<Block> {
    let empty_rels = HashMap::new();
    let empty_numbering = HashMap::new();
    let empty_images = HashMap::new();
    let no_notes = NoteDefs::default();
    let ctx = Context {
        rels: &empty_rels,
        numbering: &empty_numbering,
        images: &empty_images,
        notes: &no_notes,
    };
    let mut blocks = parse_document(xml, &ctx)
        .map(|d| d.blocks)
        .unwrap_or_default();
    if let Some(Block::Paragraph(first)) = blocks.first_mut() {
        trim_inlines(first);
    }
    blocks
}

/// Footnotes or endnotes by id; the separators Word stores are skipped.
fn parse_notes(xml: &str, element: &str) -> HashMap<String, Vec<Block>> {
    split_notes(xml, element)
        .into_iter()
        .filter(|n| n.kind.as_deref().is_none_or(|k| k == "normal"))
        .map(|n| (n.id, note_blocks(&n.xml)))
        .filter(|(_, blocks)| !blocks.is_empty())
        .collect()
}

/// Comments by id, each starting with its author and date.
fn parse_comments(xml: &str) -> HashMap<String, Vec<Block>> {
    split_notes(xml, "comment")
        .into_iter()
        .map(|n| {
            let mut blocks = note_blocks(&n.xml);
            let mut head = Vec::new();
            if let Some(author) = n.author.filter(|a| !a.is_empty()) {
                head.push(Inline::Strong(vec![Inline::Text(author)]));
            }
            if let Some(date) = n.date.as_deref().and_then(|d| d.get(..10)) {
                head.push(Inline::Text(format!(" ({date})")));
            }
            if !head.is_empty() {
                head.push(Inline::Text(": ".to_string()));
                match blocks.first_mut() {
                    Some(Block::Paragraph(first)) => {
                        first.splice(0..0, head);
                    }
                    _ => blocks.insert(0, Block::Paragraph(head)),
                }
            }
            (n.id, blocks)
        })
        .filter(|(_, blocks)| !blocks.is_empty())
        .collect()
}

/// A reference to note `id`, queuing its definition the first time.
fn note_reference(
    e: &quick_xml::events::BytesStart,
    prefix: &str,
    defs: &HashMap<String, Vec<Block>>,
    emitted: &mut HashSet<String>,
    queue: &mut Vec<Block>,
) -> Option<Inline> {
    let id = attr_value(e, "id")?;
    let content = defs.get(&id)?;
    let label = format!("{prefix}{id}");
    if emitted.insert(label.clone()) {
        queue.push(Block::Footnote {
            label: label.clone(),
            content: content.clone(),
        });
    }
    Some(Inline::FootnoteRef(label))
}

fn parse_document(xml: &str, ctx: &Context) -> Result<Document> {
    let Context {
        rels,
        numbering,
        images,
        notes,
    } = ctx;
    let mut out = Builder::default();
    let mut note_queue: Vec<Block> = Vec::new();
    let mut emitted: HashSet<String> = HashSet::new();
    // Inside w:del / w:moveFrom: tracked deletions, kept as struck-out text.
    let mut in_deletion = false;
    let mut reader = Reader::from_str(xml);

    let mut in_paragraph = false;
    let mut in_run = false;
    let mut in_table = false;
    let mut in_table_row = false;
    let mut in_table_cell = false;
    let mut current: Vec<Inline> = Vec::new();
    let mut list_counters: HashMap<(String, u32), usize> = HashMap::new();
    let mut current_style: Option<String> = None;
    let mut is_bold = false;
    let mut is_italic = false;
    let mut is_list_item = false;
    let mut current_ilvl: u32 = 0;
    let mut current_num_id: Option<String> = None;
    let mut table_rows: Vec<Vec<Vec<Inline>>> = Vec::new();
    let mut table_row: Vec<Vec<Inline>> = Vec::new();
    let mut cell: Vec<Inline> = Vec::new();
    let mut hyperlink_target: Option<String> = None;
    let mut hyperlink_buf: Vec<Inline> = Vec::new();
    let mut in_hyperlink = false;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "p" => {
                        if in_table_cell && !cell.is_empty() {
                            cell.push(Inline::Break);
                        }
                        in_paragraph = true;
                        current.clear();
                        current_style = None;
                        is_bold = false;
                        is_italic = false;
                        is_list_item = false;
                        current_ilvl = 0;
                        current_num_id = None;
                    }
                    "r" => in_run = true,
                    "del" | "moveFrom" => in_deletion = true,
                    "blip" => {
                        if let Some(image) = embedded_image(&e, images) {
                            if in_table_cell {
                                cell.push(image);
                            } else if in_paragraph {
                                current.push(image);
                            }
                        }
                    }
                    "hyperlink" => {
                        in_hyperlink = true;
                        hyperlink_buf.clear();
                        hyperlink_target = e
                            .attributes()
                            .flatten()
                            .find(|attr| attr.key.as_ref() == "r:id" || attr.key.as_ref() == "id")
                            .and_then(|attr| rels.get(attr.value.as_ref()).cloned());
                    }
                    "tbl" => {
                        in_table = true;
                        table_rows.clear();
                    }
                    "tr" => {
                        in_table_row = true;
                        table_row.clear();
                    }
                    "tc" => {
                        in_table_cell = true;
                        cell.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "pStyle" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "w:val" || attr.key.as_ref() == "val" {
                                current_style = Some(attr.value.to_string());
                            }
                        }
                    }
                    "blip" => {
                        if let Some(image) = embedded_image(&e, images) {
                            if in_table_cell {
                                cell.push(image);
                            } else if in_paragraph {
                                current.push(image);
                            }
                        }
                    }
                    "footnoteReference" | "endnoteReference" | "commentReference" => {
                        let (prefix, defs) = match local.as_str() {
                            "footnoteReference" => ("", &notes.footnotes),
                            "endnoteReference" => ("e", &notes.endnotes),
                            _ => ("c", &notes.comments),
                        };
                        if let Some(reference) =
                            note_reference(&e, prefix, defs, &mut emitted, &mut note_queue)
                        {
                            if in_table_cell {
                                cell.push(reference);
                            } else if in_hyperlink {
                                hyperlink_buf.push(reference);
                            } else if in_paragraph {
                                current.push(reference);
                            }
                        }
                    }
                    "b" => is_bold = true,
                    "i" => is_italic = true,
                    "numPr" => is_list_item = true,
                    "ilvl" => {
                        is_list_item = true;
                        current_ilvl = attr_value(&e, "val")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0);
                    }
                    "numId" => current_num_id = attr_value(&e, "val"),
                    _ => {}
                }
            }
            Ok(event @ (Event::Text(_) | Event::GeneralRef(_))) => {
                if in_run || in_table_cell {
                    // `&amp;` and friends arrive as events of their own.
                    let text = match &event {
                        Event::Text(e) => e.to_string(),
                        Event::GeneralRef(r) => match entity_text(r) {
                            Some(text) => text,
                            None => continue,
                        },
                        _ => continue,
                    };
                    if in_table_cell {
                        let run = Inline::Text(text);
                        push_run(
                            &mut cell,
                            if in_deletion {
                                Inline::Strike(vec![run])
                            } else {
                                run
                            },
                        );
                    } else if in_paragraph && !text.is_empty() {
                        let mut run = Inline::styled(text, is_bold, is_italic);
                        if in_deletion {
                            run = Inline::Strike(vec![run]);
                        }
                        if in_hyperlink {
                            push_run(&mut hyperlink_buf, run);
                        } else {
                            push_run(&mut current, run);
                        }
                    }
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "p" => {
                        if !in_table_cell && in_paragraph {
                            let content = std::mem::take(&mut current);
                            let heading = current_style.as_deref().and_then(heading_level);
                            let quote = current_style.as_deref().is_some_and(is_blockquote);
                            let is_list = is_list_item && heading.is_none() && !quote;
                            if let Some(level) = heading {
                                out.push(Block::Heading { level, content });
                            } else if quote {
                                out.push(Block::Quote(vec![Block::Paragraph(content)]));
                            } else if is_list {
                                let ordered = current_num_id.as_deref().is_some_and(|id| {
                                    numbering
                                        .get(&(id.to_string(), current_ilvl))
                                        .copied()
                                        .unwrap_or(false)
                                });
                                if ordered {
                                    let key =
                                        (current_num_id.clone().unwrap_or_default(), current_ilvl);
                                    let n = list_counters.entry(key).or_insert(0);
                                    *n += 1;
                                    out.numbered_item(current_ilvl as usize, *n, content);
                                } else {
                                    out.item(current_ilvl as usize, false, content);
                                }
                            } else if !content.is_empty() {
                                out.push(Block::Paragraph(content));
                            }
                        }
                        in_paragraph = false;
                    }
                    "r" => {
                        in_run = false;
                        is_bold = false;
                        is_italic = false;
                    }
                    "del" | "moveFrom" => in_deletion = false,
                    "hyperlink" => {
                        if in_hyperlink && !in_table_cell {
                            let content = std::mem::take(&mut hyperlink_buf);
                            match hyperlink_target.take() {
                                Some(url) => current.push(Inline::Link { url, content }),
                                None => current.extend(content),
                            }
                        }
                        in_hyperlink = false;
                        hyperlink_buf.clear();
                        hyperlink_target = None;
                    }
                    "tc" => {
                        let mut done = std::mem::take(&mut cell);
                        trim_inlines(&mut done);
                        table_row.push(done);
                        in_table_cell = false;
                    }
                    "tr" => {
                        table_rows.push(table_row.clone());
                        table_row.clear();
                        in_table_row = false;
                    }
                    "tbl" => {
                        if !table_rows.is_empty() {
                            out.push(Block::Table(table_rows.clone()));
                        }
                        table_rows.clear();
                        in_table = false;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(Error::Conversion {
                    format: "word",
                    message: format!("Failed to parse document.xml: {e}"),
                });
            }
            _ => {}
        }
    }

    // Suppress unused variable warnings
    let _ = in_table;
    let _ = in_table_row;

    let mut doc = out.finish();
    doc.extend(note_queue);
    Ok(doc)
}

/// Adds `run` to `target`, joining it to the run before when both carry the
/// same marks: a run split at an entity or by Word is still one run.
fn push_run(target: &mut Vec<Inline>, run: Inline) {
    if let Some(last) = target.last_mut()
        && merge_runs(last, &run)
    {
        return;
    }
    target.push(run);
}

fn merge_runs(last: &mut Inline, next: &Inline) -> bool {
    match (last, next) {
        (Inline::Text(a), Inline::Text(b)) => {
            a.push_str(b);
            true
        }
        (Inline::Strong(a), Inline::Strong(b))
        | (Inline::Emphasis(a), Inline::Emphasis(b))
        | (Inline::Strike(a), Inline::Strike(b)) => match (a.last_mut(), b.as_slice()) {
            (Some(last), [next]) => merge_runs(last, next),
            _ => false,
        },
        _ => false,
    }
}

/// The image for a `a:blip r:embed="rIdN"` element, if that image was extracted.
fn embedded_image(
    e: &quick_xml::events::BytesStart,
    images: &HashMap<String, String>,
) -> Option<Inline> {
    let id = e
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == "r:embed" || a.key.as_ref() == "embed")
        .map(|a| a.value.to_string())?;
    let url = images.get(&id)?.clone();
    Some(Inline::Image {
        url,
        alt: "image".to_string(),
    })
}

fn attr_value(e: &quick_xml::events::BytesStart, name: &str) -> Option<String> {
    let prefixed = format!("w:{name}");
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name || a.key.as_ref() == prefixed)
        .map(|a| a.value.to_string())
}

fn parse_numbering(xml: &str) -> HashMap<(String, u32), bool> {
    let mut reader = Reader::from_str(xml);
    let mut abstract_fmt: HashMap<(String, u32), String> = HashMap::new();
    let mut num_to_abstract: HashMap<String, String> = HashMap::new();

    let mut current_abstract_id: Option<String> = None;
    let mut current_num_id: Option<String> = None;
    let mut current_lvl: Option<u32> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "abstractNum" => current_abstract_id = attr_value(&e, "abstractNumId"),
                    "num" => current_num_id = attr_value(&e, "numId"),
                    "lvl" => {
                        current_lvl = attr_value(&e, "ilvl").and_then(|v| v.parse().ok());
                    }
                    "abstractNumId" => {
                        if let (Some(num_id), Some(val)) = (&current_num_id, attr_value(&e, "val"))
                        {
                            num_to_abstract.insert(num_id.clone(), val);
                        }
                    }
                    "numFmt" => {
                        if let (Some(abs_id), Some(lvl), Some(val)) =
                            (&current_abstract_id, current_lvl, attr_value(&e, "val"))
                        {
                            abstract_fmt.insert((abs_id.clone(), lvl), val);
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => match local_name(e.name().as_ref()).as_str() {
                "abstractNum" => current_abstract_id = None,
                "num" => current_num_id = None,
                "lvl" => current_lvl = None,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }

    let mut result = HashMap::new();
    for (num_id, abs_id) in &num_to_abstract {
        for ((a, lvl), fmt) in &abstract_fmt {
            if a == abs_id {
                result.insert((num_id.clone(), *lvl), fmt != "bullet" && fmt != "none");
            }
        }
    }
    result
}

fn is_blockquote(style: &str) -> bool {
    let lower = style.to_ascii_lowercase();
    lower == "quote" || lower == "intensequote" || lower == "blockquote"
}

fn heading_level(style: &str) -> Option<u8> {
    let lower = style.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("heading") {
        rest.trim()
            .parse::<u8>()
            .ok()
            .filter(|&n| (1..=6).contains(&n))
    } else if let Some(rest) = lower.strip_prefix("titre") {
        rest.trim()
            .parse::<u8>()
            .ok()
            .filter(|&n| (1..=6).contains(&n))
    } else {
        None
    }
}

fn read_entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String> {
    let mut file = archive.by_name(name).map_err(|e| Error::Conversion {
        format: "word",
        message: format!("Entry not found: {name}: {e}"),
    })?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

fn local_name(name: &str) -> String {
    if let Some(pos) = name.rfind(':') {
        name[pos + 1..].to_string()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::{ConvertOptions, Converter};

    fn make_docx(document_xml: &str, rels_xml: Option<&str>) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        if let Some(rels) = rels_xml {
            zip.start_file("word/_rels/document.xml.rels", options)
                .unwrap();
            zip.write_all(rels.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn doc_xml(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
            xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <w:body>{body}</w:body>
</w:document>"#
        )
    }

    fn convert(docx_bytes: &[u8]) -> String {
        let mut out = Vec::new();
        WordConverter.convert(docx_bytes, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    fn make_docx_with_numbering(document_xml: &str, numbering_xml: &str) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        zip.start_file("word/numbering.xml", options).unwrap();
        zip.write_all(numbering_xml.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn test_hyperlink_resolved_from_relationships() {
        let body =
            r#"<w:p><w:hyperlink r:id="rId1"><w:r><w:t>mq-conv</w:t></w:r></w:hyperlink></w:p>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://github.com/harehare/mq-conv" TargetMode="External"/>
</Relationships>"#;
        let docx = make_docx(&doc_xml(body), Some(rels));
        let out = convert(&docx);
        assert!(
            out.contains("[mq-conv](https://github.com/harehare/mq-conv)"),
            "expected markdown link in:\n{out}"
        );
    }

    #[test]
    fn test_hyperlink_without_relationships_falls_back_to_text() {
        let body =
            r#"<w:p><w:hyperlink r:id="rId1"><w:r><w:t>plain text</w:t></w:r></w:hyperlink></w:p>"#;
        let docx = make_docx(&doc_xml(body), None);
        let out = convert(&docx);
        assert!(out.contains("plain text"), "missing text in:\n{out}");
        assert!(
            !out.contains('['),
            "should not emit markdown link syntax:\n{out}"
        );
    }

    #[test]
    fn test_hyperlink_with_bold_run() {
        let body = r#"<w:p><w:hyperlink r:id="rId1"><w:r><w:rPr><w:b/></w:rPr><w:t>bold link</w:t></w:r></w:hyperlink></w:p>"#;
        let rels = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Target="https://example.com"/>
</Relationships>"#;
        let docx = make_docx(&doc_xml(body), Some(rels));
        let out = convert(&docx);
        assert!(
            out.contains("[**bold link**](https://example.com)"),
            "expected formatted markdown link in:\n{out}"
        );
    }

    #[test]
    fn test_plain_paragraph_unaffected() {
        let body = r#"<w:p><w:r><w:t>Hello world</w:t></w:r></w:p>"#;
        let docx = make_docx(&doc_xml(body), None);
        let out = convert(&docx);
        assert!(out.contains("Hello world"));
    }

    const NUMBERING_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:abstractNum w:abstractNumId="0">
    <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
    <w:lvl w:ilvl="1"><w:numFmt w:val="lowerLetter"/></w:lvl>
  </w:abstractNum>
  <w:abstractNum w:abstractNumId="1">
    <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
  </w:abstractNum>
  <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
  <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#;

    fn numbered_item(text: &str, num_id: u32, ilvl: u32) -> String {
        format!(
            r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{ilvl}"/><w:numId w:val="{num_id}"/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>"#
        )
    }

    #[test]
    fn test_ordered_list_numbered_sequentially() {
        let body = format!(
            "{}{}{}",
            numbered_item("First", 1, 0),
            numbered_item("Second", 1, 0),
            numbered_item("Third", 1, 0)
        );
        let docx = make_docx_with_numbering(&doc_xml(&body), NUMBERING_XML);
        let out = convert(&docx);
        assert!(out.contains("1. First"), "{out}");
        assert!(out.contains("2. Second"), "{out}");
        assert!(out.contains("3. Third"), "{out}");
    }

    #[test]
    fn test_bullet_numbering_format_stays_a_bullet() {
        let body = numbered_item("Item", 2, 0);
        let docx = make_docx_with_numbering(&doc_xml(&body), NUMBERING_XML);
        let out = convert(&docx);
        assert!(out.contains("- Item"), "{out}");
        assert!(!out.contains("1. Item"), "{out}");
    }

    #[test]
    fn test_nested_list_item_is_indented() {
        let body = format!(
            "{}{}",
            numbered_item("Top", 1, 0),
            numbered_item("Sub", 1, 1)
        );
        let docx = make_docx_with_numbering(&doc_xml(&body), NUMBERING_XML);
        let out = convert(&docx);
        assert!(out.contains("1. Top"), "{out}");
        assert!(out.contains("  1. Sub"), "{out}");
    }

    #[test]
    fn test_multi_paragraph_table_cell_joined_with_br() {
        let body = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Line one</w:t></w:r></w:p><w:p><w:r><w:t>Line two</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
        let docx = make_docx(&doc_xml(body), None);
        let out = convert(&docx);
        assert!(out.contains("Line one<br>Line two"), "{out}");
    }

    #[test]
    fn test_list_is_followed_by_a_blank_line_and_text_is_escaped() {
        let body = format!(
            "{}{}<w:p><w:r><w:t>2 * 3 and 1. not a list</w:t></w:r></w:p>",
            numbered_item("One", 2, 0),
            numbered_item("Two", 2, 0)
        );
        let docx = make_docx_with_numbering(&doc_xml(&body), NUMBERING_XML);
        assert_eq!(
            convert(&docx),
            "- One\n- Two\n\n2 \\* 3 and 1. not a list\n"
        );
    }

    #[test]
    fn test_table_cells_escape_pipes_once() {
        let body = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>H</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>a|b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
        let out = convert(&make_docx(&doc_xml(body), None));
        assert!(out.contains("| a\\|b |"), "{out}");
    }

    #[test]
    fn test_numbering_continues_after_an_interruption() {
        let body = format!(
            "{}<w:p><w:r><w:t>break</w:t></w:r></w:p>{}",
            numbered_item("One", 1, 0),
            numbered_item("Two", 1, 0)
        );
        let docx = make_docx_with_numbering(&doc_xml(&body), NUMBERING_XML);
        assert_eq!(convert(&docx), "1. One\n\nbreak\n\n2. Two\n");
    }

    #[test]
    fn test_embedded_image_is_extracted_and_referenced() {
        let body = r#"<w:p><w:r><w:t>Before</w:t></w:r></w:p>
            <w:p><w:r><w:drawing><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:blipFill><a:blip r:embed="rId5"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></w:drawing></w:r></w:p>
            <w:p><w:r><w:t>After</w:t></w:r></w:p>"#;
        let rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
            <Relationship Id="rId5" Type="image" Target="media/image1.png"/></Relationships>"#;

        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in [
            ("word/document.xml", doc_xml(body).into_bytes()),
            ("word/_rels/document.xml.rels", rels.as_bytes().to_vec()),
            ("word/media/image1.png", b"PNGDATA".to_vec()),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(&data).unwrap();
        }
        let docx = zip.finish().unwrap().into_inner();

        let dir = std::env::temp_dir().join(format!("mq-conv-docx-{}", std::process::id()));
        let options = ConvertOptions {
            image_dir: Some(dir.clone()),
            ..Default::default()
        };
        let mut out = Vec::new();
        WordConverter
            .convert_with(&docx, &mut out, &options)
            .unwrap();
        let md = String::from_utf8(out).unwrap();

        let image_line = format!("![image]({})", dir.join("image1.png").to_string_lossy());
        let (before, img, after) = (
            md.find("Before").unwrap(),
            md.find(&image_line)
                .unwrap_or_else(|| panic!("no image link in:\n{md}")),
            md.find("After").unwrap(),
        );
        assert!(before < img && img < after, "image out of place:\n{md}");
        assert_eq!(std::fs::read(dir.join("image1.png")).unwrap(), b"PNGDATA");
        std::fs::remove_dir_all(&dir).unwrap();

        // Without an image directory the image is simply omitted.
        let plain = convert(&docx);
        assert!(!plain.contains("!["), "{plain}");
    }

    fn make_docx_parts(parts: &[(&str, String)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in parts {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    const W_NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    #[test]
    fn test_tracked_changes_keep_deletions_struck_out() {
        let body = r#"<w:p><w:r><w:t xml:space="preserve">Keep </w:t></w:r><w:del w:id="1" w:author="A"><w:r><w:delText>gone</w:delText></w:r></w:del><w:ins w:id="2" w:author="A"><w:r><w:t>added</w:t></w:r></w:ins></w:p>"#;
        let out = convert(&make_docx(&doc_xml(body), None));
        assert_eq!(out, "Keep ~~gone~~added\n");
    }

    #[test]
    fn test_paragraph_mark_revision_is_not_a_deletion() {
        let body = r#"<w:p><w:pPr><w:rPr><w:del w:id="1" w:author="A"/></w:rPr></w:pPr><w:r><w:t>Still here</w:t></w:r></w:p>"#;
        let out = convert(&make_docx(&doc_xml(body), None));
        assert_eq!(out, "Still here\n");
    }

    #[test]
    fn test_comments_become_footnotes_with_author_and_date() {
        let body = r#"<w:p><w:commentRangeStart w:id="0"/><w:r><w:t>Reviewed</w:t></w:r><w:commentRangeEnd w:id="0"/><w:r><w:commentReference w:id="0"/></w:r></w:p>"#;
        let comments = format!(
            r#"<w:comments {W_NS}><w:comment w:id="0" w:author="Alice" w:date="2024-05-06T07:08:00Z"><w:p><w:r><w:t>Please check</w:t></w:r></w:p></w:comment></w:comments>"#
        );
        let docx = make_docx_parts(&[
            ("word/document.xml", doc_xml(body)),
            ("word/comments.xml", comments),
        ]);
        assert_eq!(
            convert(&docx),
            "Reviewed[^c0]\n\n[^c0]: **Alice** (2024-05-06): Please check\n"
        );
    }

    #[test]
    fn test_footnotes_and_endnotes_skip_separators() {
        let body = r#"<w:p><w:r><w:t>Claim</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:t> and more</w:t></w:r><w:r><w:endnoteReference w:id="1"/></w:r></w:p>"#;
        let footnotes = format!(
            r#"<w:footnotes {W_NS}>
<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
<w:footnote w:id="1"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t xml:space="preserve"> See the source.</w:t></w:r></w:p></w:footnote>
</w:footnotes>"#
        );
        let endnotes = format!(
            r#"<w:endnotes {W_NS}><w:endnote w:id="1"><w:p><w:r><w:t>Ended.</w:t></w:r></w:p></w:endnote></w:endnotes>"#
        );
        let docx = make_docx_parts(&[
            ("word/document.xml", doc_xml(body)),
            ("word/footnotes.xml", footnotes),
            ("word/endnotes.xml", endnotes),
        ]);
        assert_eq!(
            convert(&docx),
            "Claim[^1] and more[^e1]\n\n[^1]: See the source.\n\n[^e1]: Ended.\n"
        );
    }

    #[test]
    fn test_reference_without_a_definition_is_dropped() {
        let body =
            r#"<w:p><w:r><w:t>Text</w:t></w:r><w:r><w:footnoteReference w:id="9"/></w:r></w:p>"#;
        let out = convert(&make_docx(&doc_xml(body), None));
        assert_eq!(out, "Text\n");
    }

    #[test]
    fn test_xml_entities_in_text_are_kept() {
        let body = r#"<w:p><w:r><w:t>Tom &amp; Jerry &lt;3 &#x41;</w:t></w:r></w:p>"#;
        let out = convert(&make_docx(&doc_xml(body), None));
        assert_eq!(out, "Tom & Jerry \\<3 A\n");
    }
}
