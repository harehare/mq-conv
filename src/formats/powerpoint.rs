use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::converter::{ConvertOptions, Converter};
use crate::document::{Block, Builder, Document, Inline, trim_inlines};
use crate::error::{Error, Result};
use crate::formats::media::{
    MediaWriter, extract_related_images, parse_relationships, resolve_archive_path,
};
use crate::formats::xmlutil::entity_text;

pub struct PowerPointConverter;

impl Converter for PowerPointConverter {
    fn format_name(&self) -> &'static str {
        "powerpoint"
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
            format: "powerpoint",
            message: e.to_string(),
        })?;

        let mut slide_names: Vec<String> = Vec::new();
        for i in 0..archive.len() {
            if let Ok(entry) = archive.by_index(i) {
                let Ok(name) = entry.name().map(|n| n.into_owned()) else {
                    continue;
                };
                if name.starts_with("ppt/slides/slide") && name.ends_with(".xml") {
                    slide_names.push(name);
                }
            }
        }

        slide_names.sort_by_key(|name| {
            name.trim_start_matches("ppt/slides/slide")
                .trim_end_matches(".xml")
                .parse::<u32>()
                .unwrap_or(0)
        });

        options.check_pages("powerpoint", "slide", slide_names.len())?;

        // One writer for the whole deck so image names stay unique across slides.
        let mut media = MediaWriter::from_options(options);
        let mut doc = Document::new();

        for (idx, slide_name) in slide_names.iter().enumerate() {
            // Slide numbers stay those of the deck, whatever `--pages` skips.
            if !options.is_selected(idx as u32 + 1) {
                continue;
            }
            let xml = read_entry(&mut archive, slide_name)?;
            let content = extract_slide_content(&xml)?;

            if !doc.blocks.is_empty() {
                doc.push(Block::Rule);
            }
            let mut slide = Builder::default();

            // The first shape is the slide title when it looks like one.
            let title = content.shapes.first().filter(|s| s.is_title);
            match title {
                Some(first) => slide.push(Block::Heading {
                    level: 1,
                    content: join_paragraphs(&first.paragraphs),
                }),
                None => slide.push(Block::heading(1, format!("Slide {}", idx + 1))),
            }

            let content_shapes: Vec<_> = content.shapes[usize::from(title.is_some())..]
                .iter()
                .filter(|s| !s.paragraphs.is_empty())
                .collect();

            if content_shapes.is_empty() && content.tables.is_empty() && title.is_none() {
                slide.push(Block::emphasized("Empty slide"));
            }

            for shape in &content_shapes {
                if shape.is_subtitle {
                    let text = join_paragraphs(&shape.paragraphs);
                    if !text.is_empty() {
                        slide.push(Block::Heading {
                            level: 2,
                            content: text,
                        });
                    }
                    continue;
                }
                for para in &shape.paragraphs {
                    let mut text = paragraph_inlines(para);
                    trim_inlines(&mut text);
                    if text.is_empty() {
                        continue;
                    }
                    if para.has_bullet {
                        slide.item(para.level as usize, false, text);
                    } else {
                        slide.push(Block::Paragraph(text));
                    }
                }
            }

            for table in &content.tables {
                slide.push(Block::text_table(table));
            }

            // Pictures (only when an image directory was requested)
            if let Some(media) = media.as_mut() {
                let rels_name = slide_name.replace("ppt/slides/", "ppt/slides/_rels/") + ".rels";
                if let Ok(rels_xml) = read_entry(&mut archive, &rels_name) {
                    let rels = parse_relationships(&rels_xml);
                    let images = extract_related_images(&mut archive, "ppt/slides/", &rels, media);
                    for id in slide_image_ids(&xml) {
                        if let Some(url) = images.get(&id) {
                            slide.push(Block::Paragraph(vec![Inline::Image {
                                url: url.clone(),
                                alt: "image".to_string(),
                            }]));
                        }
                    }
                }
            }

            // Speaker notes
            let notes_name = notes_slide_path(&mut archive, slide_name);
            if let Ok(notes_xml) = read_entry(&mut archive, &notes_name) {
                let notes = extract_slide_content(&notes_xml)?;
                let mut lines: Vec<Vec<Inline>> = Vec::new();
                for para in notes.shapes.iter().flat_map(|s| &s.paragraphs) {
                    let plain = plain_text(para);
                    if plain.trim().is_empty() || plain.trim().chars().all(|c| c.is_ascii_digit()) {
                        continue;
                    }
                    let mut text = paragraph_inlines(para);
                    trim_inlines(&mut text);
                    lines.push(text);
                }
                if !lines.is_empty() {
                    let mut note = vec![
                        Inline::Strong(vec![Inline::Text("Notes".to_string())]),
                        Inline::Text(": ".to_string()),
                    ];
                    for (i, line) in lines.into_iter().enumerate() {
                        if i > 0 {
                            note.push(Inline::Break);
                        }
                        note.extend(line);
                    }
                    slide.push(Block::Quote(vec![Block::Paragraph(note)]));
                }
            }

            doc.extend(slide.finish().blocks);
        }

        doc.write_markdown(writer)
    }
}

/// The notes part of a slide: the one its relationships name, else the
/// `notesSlideN` with the slide's own number.
fn notes_slide_path(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, slide_name: &str) -> String {
    let rels_name = slide_name.replace("ppt/slides/", "ppt/slides/_rels/") + ".rels";
    let linked = read_entry(archive, &rels_name)
        .ok()
        .and_then(|xml| notes_target(&xml))
        .map(|target| resolve_archive_path("ppt/slides/", &target));
    linked.unwrap_or_else(|| slide_name.replace("ppt/slides/slide", "ppt/notesSlides/notesSlide"))
}

/// The target of the `notesSlide` relationship in a slide's `.rels` file.
fn notes_target(rels_xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(rels_xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if e.name().as_ref().ends_with("Relationship") =>
            {
                let (mut kind, mut target) = (None, None);
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        "Type" => kind = Some(attr.value.to_string()),
                        "Target" => target = Some(attr.value.to_string()),
                        _ => {}
                    }
                }
                if kind.is_some_and(|k| k.ends_with("/notesSlide")) {
                    return target;
                }
            }
            Ok(Event::Eof) | Err(_) => return None,
            _ => {}
        }
    }
}

struct SlideContent {
    shapes: Vec<SlideShape>,
    tables: Vec<Vec<Vec<String>>>,
}

struct SlideShape {
    paragraphs: Vec<Paragraph>,
    is_title: bool,
    is_subtitle: bool,
}

struct Paragraph {
    runs: Vec<TextRun>,
    level: u32,
    has_bullet: bool,
}

struct TextRun {
    text: String,
    bold: bool,
    italic: bool,
}

/// Relationship ids of the pictures (`a:blip r:embed`) in a slide, in document order.
fn slide_image_ids(xml: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut reader = Reader::from_str(xml);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(e)) | Ok(Event::Start(e))
                if local_name(e.name().as_ref()) == "blip" =>
            {
                if let Some(a) = e
                    .attributes()
                    .flatten()
                    .find(|a| a.key.as_ref() == "r:embed" || a.key.as_ref() == "embed")
                {
                    ids.push(a.value.to_string());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    ids
}

fn plain_text(para: &Paragraph) -> String {
    para.runs.iter().map(|run| run.text.as_str()).collect()
}

fn paragraph_inlines(para: &Paragraph) -> Vec<Inline> {
    para.runs
        .iter()
        .filter(|run| !run.text.is_empty())
        .map(|run| Inline::styled(run.text.clone(), run.bold, run.italic))
        .collect()
}

/// Paragraphs on one line, spaced apart except next to scripts written without spaces.
fn join_paragraphs(paragraphs: &[Paragraph]) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut last: Option<char> = None;
    for para in paragraphs {
        let text = plain_text(para);
        let Some(next) = text.chars().next() else {
            continue;
        };
        if let Some(prev) = last
            && !is_spaceless_boundary(prev, next)
        {
            out.push(Inline::Text(" ".to_string()));
        }
        out.extend(paragraph_inlines(para));
        last = text.chars().last();
    }
    out
}

fn is_spaceless_boundary(prev: char, next: char) -> bool {
    is_spaceless_script(prev) || is_spaceless_script(next)
}

fn is_spaceless_script(c: char) -> bool {
    matches!(c as u32,
        0x3000..=0x303F
        | 0x3040..=0x309F
        | 0x30A0..=0x30FF
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFF00..=0xFFEF
    )
}

fn extract_slide_content(xml: &str) -> Result<SlideContent> {
    let mut shapes = Vec::new();
    let mut tables: Vec<Vec<Vec<String>>> = Vec::new();
    let mut reader = Reader::from_str(xml);

    let mut in_shape = false;
    let mut in_text_body = false;
    let mut in_paragraph = false;
    let mut in_run = false;
    let mut in_text = false;
    let mut in_ppr = false;
    let mut in_rpr = false;
    let mut in_table = false;
    let mut in_table_row = false;
    let mut in_table_cell = false;

    let mut current_run = TextRun {
        text: String::new(),
        bold: false,
        italic: false,
    };
    let mut current_paragraph = Paragraph {
        runs: Vec::new(),
        level: 0,
        has_bullet: false,
    };
    let mut paragraphs: Vec<Paragraph> = Vec::new();
    let mut shape_type = String::new();

    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_row: Vec<String> = Vec::new();
    let mut cell_text = String::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "sp" | "pic" if !in_table => {
                        in_shape = true;
                        paragraphs.clear();
                        shape_type.clear();
                    }
                    "txBody" => in_text_body = true,
                    "p" if in_text_body => {
                        in_paragraph = true;
                        current_paragraph = Paragraph {
                            runs: Vec::new(),
                            level: 0,
                            has_bullet: false,
                        };
                    }
                    "pPr" if in_paragraph => {
                        in_ppr = true;
                        if let Some(attr) =
                            e.attributes().flatten().find(|a| a.key.as_ref() == "lvl")
                        {
                            current_paragraph.level = attr.value.parse().unwrap_or(0);
                        }
                    }
                    "r" if in_paragraph => {
                        in_run = true;
                        current_run = TextRun {
                            text: String::new(),
                            bold: false,
                            italic: false,
                        };
                    }
                    "rPr" if in_run => {
                        in_rpr = true;
                        // Check attributes for bold/italic
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                "b" => {
                                    current_run.bold =
                                        attr.value.as_ref() == "1" || attr.value.as_ref() == "true";
                                }
                                "i" => {
                                    current_run.italic =
                                        attr.value.as_ref() == "1" || attr.value.as_ref() == "true";
                                }
                                _ => {}
                            }
                        }
                    }
                    "t" if in_run => in_text = true,
                    "tbl" => {
                        in_table = true;
                        table_rows.clear();
                    }
                    "tr" if in_table => {
                        in_table_row = true;
                        table_row.clear();
                    }
                    "tc" if in_table_row => {
                        in_table_cell = true;
                        cell_text.clear();
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "ph" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == "type" {
                                shape_type = attr.value.to_string();
                            }
                        }
                        if shape_type.is_empty() {
                            shape_type = "body".to_string();
                        }
                    }
                    "pPr" if in_paragraph => {
                        if let Some(attr) =
                            e.attributes().flatten().find(|a| a.key.as_ref() == "lvl")
                        {
                            current_paragraph.level = attr.value.parse().unwrap_or(0);
                        }
                    }
                    "buChar" | "buAutoNum" | "buFont" if in_ppr => {
                        current_paragraph.has_bullet = true;
                    }
                    "rPr" if in_run => {
                        // Self-closing rPr
                        for attr in e.attributes().flatten() {
                            match attr.key.as_ref() {
                                "b" => {
                                    current_run.bold =
                                        attr.value.as_ref() == "1" || attr.value.as_ref() == "true";
                                }
                                "i" => {
                                    current_run.italic =
                                        attr.value.as_ref() == "1" || attr.value.as_ref() == "true";
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(e)) => {
                let decoded = e.to_string();
                if in_table_cell {
                    cell_text.push_str(&decoded);
                } else if in_text {
                    current_run.text.push_str(&decoded);
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if let Some(decoded) = entity_text(&e) {
                    if in_table_cell {
                        cell_text.push_str(&decoded);
                    } else if in_text {
                        current_run.text.push_str(&decoded);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let local = local_name(e.name().as_ref());
                match local.as_str() {
                    "sp" | "pic" if !in_table => {
                        if in_shape && !paragraphs.is_empty() {
                            let is_title = matches!(shape_type.as_str(), "title" | "ctrTitle");
                            let is_subtitle = matches!(shape_type.as_str(), "subTitle");
                            shapes.push(SlideShape {
                                paragraphs: std::mem::take(&mut paragraphs),
                                is_title,
                                is_subtitle,
                            });
                        }
                        in_shape = false;
                    }
                    "txBody" => in_text_body = false,
                    "p" if in_text_body && !in_table_cell => {
                        if in_paragraph && !current_paragraph.runs.is_empty() {
                            paragraphs.push(std::mem::replace(
                                &mut current_paragraph,
                                Paragraph {
                                    runs: Vec::new(),
                                    level: 0,
                                    has_bullet: false,
                                },
                            ));
                        }
                        in_paragraph = false;
                    }
                    "pPr" => in_ppr = false,
                    "r" if !in_table_cell => {
                        if in_run && !current_run.text.is_empty() {
                            current_paragraph.runs.push(std::mem::replace(
                                &mut current_run,
                                TextRun {
                                    text: String::new(),
                                    bold: false,
                                    italic: false,
                                },
                            ));
                        }
                        in_run = false;
                        in_rpr = false;
                    }
                    "rPr" => in_rpr = false,
                    "t" => in_text = false,
                    "tc" => {
                        table_row.push(cell_text.trim().to_string());
                        cell_text.clear();
                        in_table_cell = false;
                    }
                    "tr" => {
                        table_rows.push(table_row.clone());
                        table_row.clear();
                        in_table_row = false;
                    }
                    "tbl" => {
                        if !table_rows.is_empty() {
                            tables.push(table_rows.clone());
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
                    format: "powerpoint",
                    message: format!("Failed to parse slide XML: {e}"),
                });
            }
            _ => {}
        }
    }

    // Suppress unused variable warnings
    let _ = in_rpr;

    Ok(SlideContent { shapes, tables })
}

fn read_entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<String> {
    let mut file = archive.by_name(name).map_err(|e| Error::Conversion {
        format: "powerpoint",
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
    use crate::converter::Converter;
    use rstest::rstest;
    use std::io::Write;

    fn make_pptx(slides: &[(&str, &str)]) -> Vec<u8> {
        let buf = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, content) in slides {
            zip.start_file(name.to_string(), options).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn slide_xml(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
       xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"
       xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
  <p:cSld><p:spTree>{body}</p:spTree></p:cSld>
</p:sld>"#
        )
    }

    fn title_shape(text: &str) -> String {
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#
        )
    }

    fn multi_para_title_shape(paragraphs: &[&str]) -> String {
        let paras: String = paragraphs
            .iter()
            .map(|t| format!("<a:p><a:r><a:t>{t}</a:t></a:r></a:p>"))
            .collect();
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr>
<p:txBody>{paras}</p:txBody></p:sp>"#
        )
    }

    fn body_shape(text: &str) -> String {
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#
        )
    }

    fn formatted_shape(text: &str, bold: bool, italic: bool) -> String {
        let mut attrs = Vec::new();
        if bold {
            attrs.push(r#"b="1""#);
        }
        if italic {
            attrs.push(r#"i="1""#);
        }
        let rpr = if attrs.is_empty() {
            String::new()
        } else {
            format!("<a:rPr {}/>", attrs.join(" "))
        };
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r>{rpr}<a:t>{text}</a:t></a:r></a:p></p:txBody></p:sp>"#
        )
    }

    fn nested_bullet_shape(items: &[(&str, u32)]) -> String {
        let paras: String = items
            .iter()
            .map(|(t, lvl)| {
                format!(
                    r#"<a:p><a:pPr lvl="{lvl}"><a:buChar char="•"/></a:pPr><a:r><a:t>{t}</a:t></a:r></a:p>"#
                )
            })
            .collect();
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
<p:txBody>{paras}</p:txBody></p:sp>"#
        )
    }

    fn mixed_shape(plain: &str, bullet: &str) -> String {
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>{plain}</a:t></a:r></a:p><a:p><a:pPr><a:buChar char="•"/></a:pPr><a:r><a:t>{bullet}</a:t></a:r></a:p></p:txBody></p:sp>"#
        )
    }

    fn bullet_shape(items: &[&str]) -> String {
        let paras: String = items
            .iter()
            .map(|t| {
                format!(
                    r#"<a:p><a:pPr><a:buChar char="•"/></a:pPr><a:r><a:t>{t}</a:t></a:r></a:p>"#
                )
            })
            .collect();
        format!(
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="body"/></p:nvPr></p:nvSpPr>
<p:txBody>{paras}</p:txBody></p:sp>"#
        )
    }

    fn table_xml(rows: &[&[&str]]) -> String {
        let rows_xml: String = rows
            .iter()
            .map(|cells| {
                let cells_xml: String = cells
                    .iter()
                    .map(|c| {
                        format!(r#"<a:tc><a:txBody><a:p><a:r><a:t>{c}</a:t></a:r></a:p></a:txBody></a:tc>"#)
                    })
                    .collect();
                format!("<a:tr>{cells_xml}</a:tr>")
            })
            .collect();
        format!(
            r#"<a:graphicFrame><a:graphic><a:graphicData>
<a:tbl>{rows_xml}</a:tbl>
</a:graphicData></a:graphic></a:graphicFrame>"#
        )
    }

    fn convert(pptx_bytes: &[u8]) -> String {
        let converter = PowerPointConverter;
        let mut output = Vec::new();
        converter.convert(pptx_bytes, &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[rstest]
    #[case::title("title", "# Hello")]
    #[case::plain("plain", "Some content")]
    #[case::bold("bold", "**important**")]
    #[case::italic("italic", "*emphasis*")]
    #[case::bold_italic("bold_italic", "***strong***")]
    fn test_text_formatting(#[case] kind: &str, #[case] expected: &str) {
        let shape = match kind {
            "title" => title_shape("Hello"),
            "plain" => body_shape("Some content"),
            "bold" => formatted_shape("important", true, false),
            "italic" => formatted_shape("emphasis", false, true),
            "bold_italic" => formatted_shape("strong", true, true),
            _ => unreachable!(),
        };
        let xml = slide_xml(&shape);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(
            output.contains(expected),
            "Expected {expected:?} in:\n{output}"
        );
    }

    #[rstest]
    fn test_bullet_list() {
        let shape = bullet_shape(&["Item A", "Item B", "Item C"]);
        let xml = slide_xml(&shape);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("- Item A"));
        assert!(output.contains("- Item B"));
        assert!(output.contains("- Item C"));
    }

    #[rstest]
    fn test_nested_bullet_indented() {
        let shape = nested_bullet_shape(&[("Top", 0), ("Sub", 1)]);
        let xml = slide_xml(&shape);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("- Top"), "{output}");
        assert!(output.contains("  - Sub"), "{output}");
    }

    #[rstest]
    fn test_mixed_plain_and_bullet_in_same_shape() {
        let shape = mixed_shape("Overview:", "First point");
        let xml = slide_xml(&shape);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("Overview:"), "{output}");
        assert!(!output.contains("- Overview:"), "{output}");
        assert!(output.contains("- First point"), "{output}");
    }

    #[rstest]
    fn test_table() {
        let tbl = table_xml(&[&["Name", "Age"], &["Alice", "30"], &["Bob", "25"]]);
        let xml = slide_xml(&tbl);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(
            output.contains("| Name  | Age |"),
            "Missing header in:\n{output}"
        );
        assert!(
            output.contains("| ----- | --- |"),
            "Missing separator in:\n{output}"
        );
        assert!(
            output.contains("| Alice | 30  |"),
            "Missing row in:\n{output}"
        );
        assert!(
            output.contains("| Bob   | 25  |"),
            "Missing row in:\n{output}"
        );
    }

    #[rstest]
    fn test_multiple_slides() {
        let s1 = slide_xml(&title_shape("Slide One"));
        let s2 = slide_xml(&title_shape("Slide Two"));
        let pptx = make_pptx(&[
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/slide2.xml", &s2),
        ]);
        let output = convert(&pptx);
        assert!(output.contains("# Slide One"));
        assert!(output.contains("---"));
        assert!(output.contains("# Slide Two"));
    }

    #[rstest]
    fn test_speaker_notes_are_a_quote_with_breaks() {
        let slide = slide_xml(&title_shape("Deck"));
        let notes = slide_xml(&format!(
            "{}{}",
            body_shape("Say hello"),
            body_shape("Then 2 * 3")
        ));
        let pptx = make_pptx(&[
            ("ppt/slides/slide1.xml", &slide),
            ("ppt/notesSlides/notesSlide1.xml", &notes),
        ]);
        assert_eq!(
            convert(&pptx),
            "# Deck\n\n> **Notes**: Say hello  \n> Then 2 \\* 3\n"
        );
    }

    #[rstest]
    fn test_speaker_notes_follow_the_slide_relationship() {
        // Slide 1 owns notesSlide2 and slide 2 owns notesSlide1: the file
        // numbers do not line up, so the names alone would swap them.
        let s1 = slide_xml(&title_shape("First"));
        let s2 = slide_xml(&title_shape("Second"));
        let n1 = slide_xml(&body_shape("for second"));
        let n2 = slide_xml(&body_shape("for first"));
        let rels = |notes: &str| {
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide" Target="../notesSlides/{notes}"/></Relationships>"#
            )
        };
        let (r1, r2) = (rels("notesSlide2.xml"), rels("notesSlide1.xml"));
        let pptx = make_pptx(&[
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/slide2.xml", &s2),
            ("ppt/slides/_rels/slide1.xml.rels", &r1),
            ("ppt/slides/_rels/slide2.xml.rels", &r2),
            ("ppt/notesSlides/notesSlide1.xml", &n1),
            ("ppt/notesSlides/notesSlide2.xml", &n2),
        ]);
        assert_eq!(
            convert(&pptx),
            "# First\n\n> **Notes**: for first\n\n---\n\n# Second\n\n> **Notes**: for second\n"
        );
    }

    #[rstest]
    fn test_list_then_paragraph_and_slide_break() {
        let s1 = slide_xml(&format!(
            "{}{}",
            title_shape("A"),
            bullet_shape(&["x", "y"])
        ));
        let s2 = slide_xml(&format!("{}{}", title_shape("B"), body_shape("text")));
        let pptx = make_pptx(&[
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/slide2.xml", &s2),
        ]);
        assert_eq!(convert(&pptx), "# A\n\n- x\n- y\n\n---\n\n# B\n\ntext\n");
    }

    #[rstest]
    fn test_empty_slide() {
        let xml = slide_xml("");
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("*Empty slide*"));
    }

    #[rstest]
    fn test_slide_ordering() {
        let s1 = slide_xml(&title_shape("First"));
        let s2 = slide_xml(&title_shape("Second"));
        let s3 = slide_xml(&title_shape("Third"));
        let pptx = make_pptx(&[
            ("ppt/slides/slide3.xml", &s3),
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/slide2.xml", &s2),
        ]);
        let output = convert(&pptx);
        let p1 = output.find("# First").unwrap();
        let p2 = output.find("# Second").unwrap();
        let p3 = output.find("# Third").unwrap();
        assert!(p1 < p2);
        assert!(p2 < p3);
    }

    #[rstest]
    fn test_cjk_title_paragraphs_join_without_space() {
        let shape = multi_para_title_shape(&["日本語の", "タイトルです"]);
        let xml = slide_xml(&shape);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(
            output.contains("# 日本語のタイトルです"),
            "CJK paragraphs should join without an inserted space, got:\n{output}"
        );
    }

    #[rstest]
    fn test_latin_title_paragraphs_join_with_space() {
        let shape = multi_para_title_shape(&["Hello", "World"]);
        let xml = slide_xml(&shape);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(
            output.contains("# Hello World"),
            "Latin paragraphs should still join with a space, got:\n{output}"
        );
    }

    #[rstest]
    fn test_subtitle() {
        let shapes = format!(
            "{}{}",
            title_shape("Main Title"),
            r#"<p:sp><p:nvSpPr><p:nvPr><p:ph type="subTitle"/></p:nvPr></p:nvSpPr>
<p:txBody><a:p><a:r><a:t>Sub Title</a:t></a:r></a:p></p:txBody></p:sp>"#
        );
        let xml = slide_xml(&shapes);
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &xml)]);
        let output = convert(&pptx);
        assert!(output.contains("# Main Title"));
        assert!(output.contains("## Sub Title"));
    }

    fn pic_slide(rid: &str) -> String {
        slide_xml(&format!(
            r#"<p:pic><p:blipFill><a:blip r:embed="{rid}"/></p:blipFill></p:pic>"#
        ))
    }

    fn rels_to(target: &str) -> String {
        format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="image" Target="{target}"/></Relationships>"#
        )
    }

    fn convert_with_images(pptx: &[u8], dir: &std::path::Path) -> String {
        let options = ConvertOptions {
            image_dir: Some(dir.to_path_buf()),
            ..Default::default()
        };
        let mut out = Vec::new();
        PowerPointConverter
            .convert_with(pptx, &mut out, &options)
            .unwrap();
        String::from_utf8(out).unwrap()
    }

    fn image_links(md: &str) -> Vec<String> {
        md.lines()
            .filter_map(|l| l.strip_prefix("![image](")?.strip_suffix(')'))
            .map(String::from)
            .collect()
    }

    #[test]
    fn slides_with_same_named_images_keep_their_own_image() {
        let (s1, s2) = (pic_slide("rId1"), pic_slide("rId1"));
        let (r1, r2) = (
            rels_to("../media/image1.png"),
            rels_to("../other/image1.png"),
        );
        let pptx = make_pptx(&[
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/_rels/slide1.xml.rels", &r1),
            ("ppt/slides/slide2.xml", &s2),
            ("ppt/slides/_rels/slide2.xml.rels", &r2),
            ("ppt/media/image1.png", "CHART"),
            ("ppt/other/image1.png", "MAP"),
        ]);
        let dir = std::env::temp_dir().join(format!("mq-conv-pptx-names-{}", std::process::id()));
        let md = convert_with_images(&pptx, &dir);

        let links = image_links(&md);
        assert_eq!(links.len(), 2, "{md}");
        assert_ne!(links[0], links[1], "both slides link one file:\n{md}");
        let read = |link: &str| std::fs::read_to_string(link).unwrap();
        assert_eq!(
            (read(&links[0]).as_str(), read(&links[1]).as_str()),
            ("CHART", "MAP")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_image_shared_by_slides_is_written_once() {
        let s = pic_slide("rId1");
        let r = rels_to("../media/image1.png");
        let pptx = make_pptx(&[
            ("ppt/slides/slide1.xml", &s),
            ("ppt/slides/_rels/slide1.xml.rels", &r),
            ("ppt/slides/slide2.xml", &s),
            ("ppt/slides/_rels/slide2.xml.rels", &r),
            ("ppt/media/image1.png", "LOGO"),
        ]);
        let dir = std::env::temp_dir().join(format!("mq-conv-pptx-shared-{}", std::process::id()));
        let md = convert_with_images(&pptx, &dir);

        let links = image_links(&md);
        assert_eq!(links.len(), 2, "{md}");
        assert_eq!(links[0], links[1], "{md}");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn convert_pages(pptx: &[u8], pages: &str) -> Result<String> {
        let options = ConvertOptions {
            pages: Some(pages.parse().unwrap()),
            ..Default::default()
        };
        let mut out = Vec::new();
        PowerPointConverter.convert_with(pptx, &mut out, &options)?;
        Ok(String::from_utf8(out).unwrap())
    }

    fn three_slides() -> Vec<u8> {
        let s1 = slide_xml(&format!("{}{}", title_shape("First"), body_shape("one")));
        let s2 = slide_xml(&body_shape("two"));
        let s3 = slide_xml(&body_shape("three"));
        make_pptx(&[
            ("ppt/slides/slide1.xml", &s1),
            ("ppt/slides/slide2.xml", &s2),
            ("ppt/slides/slide3.xml", &s3),
        ])
    }

    #[test]
    fn pages_select_slides_and_keep_their_numbers() {
        let pptx = three_slides();

        // The first emitted slide has no leading separator, and an untitled
        // slide is still called by its position in the deck.
        let out = convert_pages(&pptx, "3").unwrap();
        assert!(out.starts_with("# Slide 3"), "{out}");
        assert!(
            out.contains("three") && !out.contains("two") && !out.contains("First"),
            "{out}"
        );

        let out = convert_pages(&pptx, "1,3").unwrap();
        assert!(out.starts_with("# First"), "{out}");
        assert_eq!(out.matches("\n---\n").count(), 1, "{out}");
        assert!(out.contains("# Slide 3") && !out.contains("two"), "{out}");
    }

    #[test]
    fn pages_outside_the_deck_are_an_error() {
        let err = convert_pages(&three_slides(), "4-")
            .unwrap_err()
            .to_string();
        assert!(err.contains("no slide matches"), "{err}");
    }

    #[rstest]
    fn test_xml_entities_in_text_are_kept() {
        let slide = slide_xml(&title_shape("R&amp;D &lt;plan&gt;"));
        let pptx = make_pptx(&[("ppt/slides/slide1.xml", &slide)]);
        assert_eq!(convert(&pptx), "# R&D \\<plan>\n");
    }
}
