//! OpenDocument text (`.odt`) and presentations (`.odp`) → Markdown.
//!
//! `content.xml` is read into a small tree and walked into the blocks of the
//! shared document model. Spreadsheets (`.ods`) are handled by the Excel
//! converter.

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::XmlVersion;
use quick_xml::events::{BytesStart, Event};

use crate::converter::{ConvertOptions, Converter};
use crate::document::{Block, Document, Inline, plain_text, trim_inlines};
use crate::error::{Error, Result};
use crate::formats::media::MediaWriter;
use crate::formats::xmlutil::entity_text;

pub struct OdtConverter;
pub struct OdpConverter;

fn err(format: &'static str, message: impl ToString) -> Error {
    Error::Conversion {
        format,
        message: message.to_string(),
    }
}

impl Converter for OdtConverter {
    fn format_name(&self) -> &'static str {
        "odt"
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
        let (mut walker, content) = Walker::open(input, "odt", options)?;
        let text = content
            .find("office:body")
            .and_then(|b| b.find("office:text"))
            .ok_or_else(|| err("odt", "the document has no text body"))?;
        let mut doc = Document::new();
        doc.extend(walker.blocks(text));
        doc.extend(walker.defs);
        doc.write_markdown(writer)
    }
}

impl Converter for OdpConverter {
    fn format_name(&self) -> &'static str {
        "odp"
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
        let (mut walker, content) = Walker::open(input, "odp", options)?;
        let presentation = content
            .find("office:body")
            .and_then(|b| b.find("office:presentation"))
            .ok_or_else(|| err("odp", "the document has no presentation body"))?;
        let slides: Vec<&Elem> = presentation
            .elems()
            .filter(|e| e.name == "draw:page")
            .collect();
        options.check_pages("odp", "slide", slides.len())?;

        let mut doc = Document::new();
        for (idx, slide) in slides.iter().enumerate() {
            // Slide numbers stay those of the deck, whatever `--pages` skips.
            if !options.is_selected(idx as u32 + 1) {
                continue;
            }
            if !doc.blocks.is_empty() {
                doc.push(Block::Rule);
            }
            doc.extend(walker.slide(slide, idx + 1));
        }
        doc.extend(walker.defs);
        doc.write_markdown(writer)
    }
}

// ---------------------------------------------------------------------------
// XML tree
// ---------------------------------------------------------------------------

/// An element with its attributes and children. Names keep their prefix
/// (`text:p`); OpenDocument fixes the prefixes it uses.
struct Elem {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<Node>,
}

enum Node {
    Elem(Elem),
    Text(String),
}

impl Elem {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn elems(&self) -> impl Iterator<Item = &Elem> {
        self.children.iter().filter_map(|c| match c {
            Node::Elem(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    fn find(&self, name: &str) -> Option<&Elem> {
        self.elems().find(|e| e.name == name)
    }

    /// All text below this element.
    fn text(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            match child {
                Node::Text(t) => out.push_str(t),
                Node::Elem(e) => out.push_str(&e.text()),
            }
        }
        out
    }
}

/// Deeper than any real document; stops hostile input from exhausting the stack.
const MAX_DEPTH: usize = 128;

fn parse_tree(xml: &str, format: &'static str) -> Result<Elem> {
    fn element(e: &BytesStart) -> Elem {
        Elem {
            name: e.name().as_ref().to_string(),
            attrs: e
                .attributes()
                .flatten()
                .map(|a| {
                    let value = a
                        .normalized_value(XmlVersion::Implicit1_0)
                        .map_or_else(|_| a.value.to_string(), |v| v.into_owned());
                    (a.key.as_ref().to_string(), value)
                })
                .collect(),
            children: Vec::new(),
        }
    }
    fn push_text(parent: &mut Elem, text: &str) {
        if let Some(Node::Text(last)) = parent.children.last_mut() {
            last.push_str(text);
        } else {
            parent.children.push(Node::Text(text.to_string()));
        }
    }

    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Elem> = vec![Elem {
        name: String::new(),
        attrs: Vec::new(),
        children: Vec::new(),
    }];
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if stack.len() > MAX_DEPTH {
                    return Err(err(format, "the XML is nested too deeply"));
                }
                stack.push(element(&e));
            }
            Ok(Event::Empty(e)) => {
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Node::Elem(element(&e)));
                }
            }
            Ok(Event::End(_)) => {
                if stack.len() > 1 {
                    let done = stack.pop().expect("checked");
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(Node::Elem(done));
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(parent) = stack.last_mut() {
                    push_text(parent, &t);
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(parent) = stack.last_mut() {
                    push_text(parent, &t);
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if let (Some(parent), Some(text)) = (stack.last_mut(), entity_text(&r)) {
                    push_text(parent, &text);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(err(format, format!("invalid XML: {e}"))),
            _ => {}
        }
    }
    // Unclosed elements (truncated input) still hold what was read.
    while stack.len() > 1 {
        let done = stack.pop().expect("checked");
        if let Some(parent) = stack.last_mut() {
            parent.children.push(Node::Elem(done));
        }
    }
    let document = stack.pop().expect("root");
    document
        .children
        .into_iter()
        .find_map(|c| match c {
            Node::Elem(e) => Some(e),
            Node::Text(_) => None,
        })
        .ok_or_else(|| err(format, "the XML has no root element"))
}

// ---------------------------------------------------------------------------
// Styles
// ---------------------------------------------------------------------------

#[derive(Default)]
struct StyleDef {
    parent: Option<String>,
    bold: Option<bool>,
    italic: Option<bool>,
    strike: Option<bool>,
}

#[derive(Default)]
struct Styles {
    styles: HashMap<String, StyleDef>,
    /// Per list style, whether each level (index 0 is level 1) is numbered.
    lists: HashMap<String, Vec<bool>>,
}

#[derive(PartialEq)]
enum ParagraphKind {
    Normal,
    Quote,
    Code,
}

impl Styles {
    fn read(&mut self, root: &Elem) {
        for container in root.elems() {
            if !matches!(
                container.name.as_str(),
                "office:styles" | "office:automatic-styles"
            ) {
                continue;
            }
            for style in container.elems() {
                match style.name.as_str() {
                    "style:style" => self.read_style(style),
                    "text:list-style" => self.read_list_style(style),
                    _ => {}
                }
            }
        }
    }

    fn read_style(&mut self, style: &Elem) {
        let Some(name) = style.attr("style:name") else {
            return;
        };
        let mut def = StyleDef {
            parent: style.attr("style:parent-style-name").map(str::to_string),
            ..Default::default()
        };
        if let Some(props) = style.find("style:text-properties") {
            def.bold = props
                .attr("fo:font-weight")
                .map(|w| w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600));
            def.italic = props
                .attr("fo:font-style")
                .map(|s| matches!(s, "italic" | "oblique"));
            def.strike = props
                .attr("style:text-line-through-style")
                .map(|s| s != "none");
        }
        self.styles.insert(name.to_string(), def);
    }

    fn read_list_style(&mut self, style: &Elem) {
        let Some(name) = style.attr("style:name") else {
            return;
        };
        let mut levels: Vec<bool> = Vec::new();
        for level in style.elems() {
            let Some(n) = level
                .attr("text:level")
                .and_then(|l| l.parse::<usize>().ok())
                .filter(|n| (1..=32).contains(n))
            else {
                continue;
            };
            if levels.len() < n {
                levels.resize(n, false);
            }
            levels[n - 1] = level.name == "text:list-level-style-number";
        }
        self.lists.insert(name.to_string(), levels);
    }

    /// The style and its ancestors, nearest first.
    fn chain<'a>(&'a self, name: &'a str) -> Vec<&'a str> {
        let mut out = vec![name];
        let mut current = name;
        while let Some(parent) = self
            .styles
            .get(current)
            .and_then(|s| s.parent.as_deref())
            .filter(|_| out.len() < 16)
        {
            out.push(parent);
            current = parent;
        }
        out
    }

    /// Bold, italic and strike-through of a text style, inherited.
    fn marks(&self, name: &str) -> (bool, bool, bool) {
        let chain = self.chain(name);
        let first = |get: fn(&StyleDef) -> Option<bool>| {
            chain
                .iter()
                .filter_map(|n| self.styles.get(*n))
                .find_map(get)
                .unwrap_or(false)
        };
        (first(|s| s.bold), first(|s| s.italic), first(|s| s.strike))
    }

    fn paragraph_kind(&self, name: &str) -> ParagraphKind {
        for style in self.chain(name) {
            match style {
                "Quotations" => return ParagraphKind::Quote,
                "Preformatted_20_Text" => return ParagraphKind::Code,
                _ => {}
            }
        }
        ParagraphKind::Normal
    }

    fn list_is_numbered(&self, name: &str, level: usize) -> bool {
        self.lists
            .get(name)
            .and_then(|levels| levels.get(level - 1))
            .copied()
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Walker
// ---------------------------------------------------------------------------

struct Walker<'a> {
    styles: Styles,
    archive: zip::ZipArchive<Cursor<&'a [u8]>>,
    media: Option<MediaWriter>,
    /// Definitions of the notes and comments referenced so far.
    defs: Vec<Block>,
    footnotes: usize,
    endnotes: usize,
    comments: usize,
    /// Blocks met inside a paragraph (text boxes), written after it.
    pending: Vec<Block>,
}

impl<'a> Walker<'a> {
    fn open(
        input: &'a [u8],
        format: &'static str,
        options: &ConvertOptions,
    ) -> Result<(Self, Elem)> {
        let mut archive = zip::ZipArchive::new(Cursor::new(input)).map_err(|e| err(format, e))?;
        let content_xml = read_entry(&mut archive, "content.xml", format)?;
        let content = parse_tree(&content_xml, format)?;
        let mut styles = Styles::default();
        if let Ok(xml) = read_entry(&mut archive, "styles.xml", format)
            && let Ok(tree) = parse_tree(&xml, format)
        {
            styles.read(&tree);
        }
        // Automatic styles of the document win over those of the same name.
        styles.read(&content);
        let walker = Walker {
            styles,
            archive,
            media: MediaWriter::from_options(options),
            defs: Vec::new(),
            footnotes: 0,
            endnotes: 0,
            comments: 0,
            pending: Vec::new(),
        };
        Ok((walker, content))
    }

    // ---- blocks ----------------------------------------------------------

    fn blocks(&mut self, e: &Elem) -> Vec<Block> {
        let mut out = Vec::new();
        for child in e.elems() {
            self.block(child, &mut out);
        }
        out
    }

    fn block(&mut self, e: &Elem, out: &mut Vec<Block>) {
        match e.name.as_str() {
            "text:h" => {
                let level = e
                    .attr("text:outline-level")
                    .and_then(|l| l.parse::<u8>().ok())
                    .unwrap_or(1)
                    .clamp(1, 6);
                let mut content = self.inlines(e);
                trim_inlines(&mut content);
                if !content.is_empty() {
                    out.push(Block::Heading { level, content });
                }
                out.append(&mut self.pending);
            }
            "text:p" => self.paragraph(e, out),
            "text:list" => out.extend(self.list(e, None, 1)),
            "table:table" => out.extend(self.table(e)),
            "draw:frame" => self.frame(e, out),
            "draw:image" => {
                if let Some(image) = self.image(e, None) {
                    out.push(Block::Paragraph(vec![image]));
                }
            }
            // Generated content and bookkeeping, not the author's text.
            "text:table-of-content"
            | "text:illustration-index"
            | "text:table-index"
            | "text:object-index"
            | "text:user-index"
            | "text:alphabetical-index"
            | "text:bibliography"
            | "text:tracked-changes"
            | "text:sequence-decls"
            | "text:variable-decls"
            | "text:user-field-decls"
            | "office:forms"
            | "office:annotation"
            | "svg:title"
            | "svg:desc" => {}
            // Sections, groups, text boxes and the like.
            _ => {
                for child in e.elems() {
                    self.block(child, out);
                }
            }
        }
    }

    fn paragraph(&mut self, e: &Elem, out: &mut Vec<Block>) {
        let kind = e
            .attr("text:style-name")
            .map_or(ParagraphKind::Normal, |s| self.styles.paragraph_kind(s));
        let mut content = self.inlines(e);
        trim_inlines(&mut content);
        if !content.is_empty() {
            out.push(match kind {
                ParagraphKind::Quote => Block::Quote(vec![Block::Paragraph(content)]),
                ParagraphKind::Code => Block::Code {
                    lang: None,
                    text: plain_text(&content),
                },
                ParagraphKind::Normal => Block::Paragraph(content),
            });
        }
        out.append(&mut self.pending);
    }

    /// A list; nested lists without a style of their own use the outer one.
    fn list(&mut self, e: &Elem, inherited: Option<&str>, level: usize) -> Option<Block> {
        let style = e.attr("text:style-name").or(inherited);
        let ordered = style.is_some_and(|s| self.styles.list_is_numbered(s, level));
        let mut items = Vec::new();
        for item in e
            .elems()
            .filter(|c| matches!(c.name.as_str(), "text:list-item" | "text:list-header"))
        {
            let mut blocks = Vec::new();
            for child in item.elems() {
                if child.name == "text:list" {
                    blocks.extend(self.list(child, style, level + 1));
                } else {
                    self.block(child, &mut blocks);
                }
            }
            if !blocks.is_empty() {
                items.push(blocks);
            }
        }
        (!items.is_empty()).then_some(Block::List {
            ordered,
            start: 1,
            items,
        })
    }

    fn table(&mut self, e: &Elem) -> Option<Block> {
        let mut rows: Vec<Vec<Vec<Inline>>> = Vec::new();
        self.table_rows(e, &mut rows);
        for row in &mut rows {
            while row.last().is_some_and(Vec::is_empty) {
                row.pop();
            }
        }
        rows.retain(|r| !r.is_empty());
        (!rows.is_empty()).then_some(Block::Table(rows))
    }

    fn table_rows(&mut self, e: &Elem, rows: &mut Vec<Vec<Vec<Inline>>>) {
        for child in e.elems() {
            match child.name.as_str() {
                "table:table-header-rows" | "table:table-row-group" | "table:table-rows" => {
                    self.table_rows(child, rows);
                }
                "table:table-row" => {
                    let mut row: Vec<Vec<Inline>> = Vec::new();
                    for cell in child.elems() {
                        let covered = cell.name == "table:covered-table-cell";
                        if cell.name != "table:table-cell" && !covered {
                            continue;
                        }
                        let content = if covered {
                            Vec::new()
                        } else {
                            self.cell_inlines(cell)
                        };
                        // Spreadsheet-style files repeat empty cells to the edge.
                        let repeat = repeat_of(cell, "table:number-columns-repeated");
                        let cap = if content.is_empty() { 64 } else { 16 };
                        for _ in 0..repeat.min(cap) {
                            row.push(content.clone());
                        }
                    }
                    let repeat = repeat_of(child, "table:number-rows-repeated").min(50);
                    for _ in 0..repeat {
                        rows.push(row.clone());
                    }
                }
                _ => {}
            }
        }
    }

    /// The paragraphs of a cell joined with line breaks.
    fn cell_inlines(&mut self, cell: &Elem) -> Vec<Inline> {
        let mut out = Vec::new();
        for (i, mut paragraph) in self.paragraphs_of(cell).into_iter().enumerate() {
            if i > 0 {
                out.push(Inline::Break);
            }
            out.append(&mut paragraph);
        }
        out
    }

    /// The text paragraphs below `e`, lists and nested tables flattened.
    fn paragraphs_of(&mut self, e: &Elem) -> Vec<Vec<Inline>> {
        let mut out = Vec::new();
        for child in e.elems() {
            match child.name.as_str() {
                "text:p" | "text:h" => {
                    let mut content = self.inlines(child);
                    trim_inlines(&mut content);
                    if !content.is_empty() {
                        out.push(content);
                    }
                    // Text boxes inside a cell have nowhere to go; keep the text.
                    for block in std::mem::take(&mut self.pending) {
                        if let Block::Paragraph(content) = block {
                            out.push(content);
                        }
                    }
                }
                "office:annotation" | "svg:title" | "svg:desc" => {}
                _ => out.extend(self.paragraphs_of(child)),
            }
        }
        out
    }

    /// A frame holds a text box, an image or a table.
    fn frame(&mut self, e: &Elem, out: &mut Vec<Block>) {
        for child in e.elems() {
            match child.name.as_str() {
                "draw:image" => {
                    if let Some(image) = self.image(child, Some(e)) {
                        out.push(Block::Paragraph(vec![image]));
                    }
                }
                _ => self.block(child, out),
            }
        }
    }

    /// The image of a `draw:image`, saved when an image directory was given.
    fn image(&mut self, image: &Elem, frame: Option<&Elem>) -> Option<Inline> {
        let media = self.media.as_mut()?;
        let href = image.attr("xlink:href")?.trim_start_matches("./");
        if href.is_empty() || href.contains("://") {
            return None;
        }
        let url = match media.saved_path(href) {
            Some(url) => url.to_string(),
            None => {
                let mut data = Vec::new();
                self.archive
                    .by_name(href)
                    .ok()?
                    .read_to_end(&mut data)
                    .ok()?;
                media.save(href, &data)?
            }
        };
        let alt = frame
            .and_then(|f| f.find("svg:title").or_else(|| f.find("svg:desc")))
            .map(|t| t.text().trim().to_string())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| "image".to_string());
        Some(Inline::Image { url, alt })
    }

    // ---- inline content --------------------------------------------------

    fn inlines(&mut self, e: &Elem) -> Vec<Inline> {
        let mut out = Vec::new();
        for child in &e.children {
            match child {
                Node::Text(t) => {
                    let text = collapse_whitespace(t);
                    if !text.is_empty() {
                        out.push(Inline::Text(text));
                    }
                }
                Node::Elem(c) => self.inline_elem(c, &mut out),
            }
        }
        out
    }

    fn inline_elem(&mut self, e: &Elem, out: &mut Vec<Inline>) {
        match e.name.as_str() {
            "text:span" => {
                let inner = self.inlines(e);
                if inner.is_empty() {
                    return;
                }
                let (bold, italic, strike) = e
                    .attr("text:style-name")
                    .map_or((false, false, false), |s| self.styles.marks(s));
                let mut run = inner;
                if italic {
                    run = vec![Inline::Emphasis(run)];
                }
                if bold {
                    run = vec![Inline::Strong(run)];
                }
                if strike {
                    run = vec![Inline::Strike(run)];
                }
                out.extend(run);
            }
            "text:a" => {
                let content = self.inlines(e);
                match e.attr("xlink:href").filter(|h| !h.is_empty()) {
                    Some(url) => out.push(Inline::Link {
                        url: url.to_string(),
                        content,
                    }),
                    None => out.extend(content),
                }
            }
            "text:s" => {
                let n = e
                    .attr("text:c")
                    .and_then(|c| c.parse::<usize>().ok())
                    .unwrap_or(1)
                    .min(256);
                out.push(Inline::Text(" ".repeat(n)));
            }
            "text:tab" => out.push(Inline::Text(" ".to_string())),
            "text:line-break" => out.push(Inline::Break),
            // Only the base text; the ruby text above it would repeat it.
            "text:ruby" => {
                if let Some(base) = e.find("text:ruby-base") {
                    out.extend(self.inlines(base));
                }
            }
            "text:note" => {
                if let Some(reference) = self.note(e) {
                    out.push(reference);
                }
            }
            "office:annotation" => {
                if let Some(reference) = self.annotation(e) {
                    out.push(reference);
                }
            }
            "draw:frame" => {
                for child in e.elems() {
                    match child.name.as_str() {
                        "draw:image" => out.extend(self.image(child, Some(e))),
                        // A text box cannot sit inside a line; it follows the paragraph.
                        "draw:text-box" => {
                            let blocks = self.blocks(child);
                            self.pending.extend(blocks);
                        }
                        _ => {}
                    }
                }
            }
            "table:table" => {
                if let Some(table) = self.table(e) {
                    self.pending.push(table);
                }
            }
            // Fields, bookmarks, tracked-change marks: their text, if any.
            _ => out.extend(self.inlines(e)),
        }
    }

    /// A footnote or endnote: a reference here, the text at the end.
    fn note(&mut self, e: &Elem) -> Option<Inline> {
        let body = e.find("text:note-body")?;
        let label = if e.attr("text:note-class") == Some("endnote") {
            self.endnotes += 1;
            format!("e{}", self.endnotes)
        } else {
            self.footnotes += 1;
            self.footnotes.to_string()
        };
        let content = self.blocks(body);
        if content.is_empty() {
            return None;
        }
        self.defs.push(Block::Footnote {
            label: label.clone(),
            content,
        });
        Some(Inline::FootnoteRef(label))
    }

    /// A comment, with its author and date in front of the text.
    fn annotation(&mut self, e: &Elem) -> Option<Inline> {
        let mut content = Vec::new();
        for child in e.elems() {
            if !matches!(child.name.as_str(), "dc:creator" | "dc:date") {
                self.block(child, &mut content);
            }
        }
        if content.is_empty() {
            return None;
        }
        let mut head = Vec::new();
        if let Some(author) = e.find("dc:creator").map(|c| c.text().trim().to_string())
            && !author.is_empty()
        {
            head.push(Inline::Strong(vec![Inline::Text(author)]));
        }
        if let Some(date) = e
            .find("dc:date")
            .map(|d| d.text())
            .and_then(|d| d.trim().get(..10).map(str::to_string))
        {
            head.push(Inline::Text(format!(" ({date})")));
        }
        if !head.is_empty() {
            head.push(Inline::Text(": ".to_string()));
            match content.first_mut() {
                Some(Block::Paragraph(first)) => {
                    first.splice(0..0, head);
                }
                _ => content.insert(0, Block::Paragraph(head)),
            }
        }
        self.comments += 1;
        let label = format!("c{}", self.comments);
        self.defs.push(Block::Footnote {
            label: label.clone(),
            content,
        });
        Some(Inline::FootnoteRef(label))
    }

    // ---- presentations ---------------------------------------------------

    /// One slide: its title, the body and the speaker notes.
    fn slide(&mut self, page: &Elem, number: usize) -> Vec<Block> {
        let mut title: Option<Vec<Inline>> = None;
        let mut subtitle: Option<Vec<Inline>> = None;
        let mut body = Vec::new();
        let mut notes: Vec<Vec<Inline>> = Vec::new();

        for shape in page.elems() {
            if shape.name == "presentation:notes" {
                for frame in shape.elems() {
                    if frame.attr("presentation:class") == Some("notes") {
                        notes.extend(self.paragraphs_of(frame));
                    }
                }
                continue;
            }
            match shape.attr("presentation:class") {
                Some("title") if title.is_none() => {
                    title = Some(self.joined_paragraphs(shape));
                }
                Some("subtitle") if subtitle.is_none() => {
                    subtitle = Some(self.joined_paragraphs(shape));
                }
                _ => self.block(shape, &mut body),
            }
        }

        let title = title.filter(|t| !t.is_empty());
        let subtitle = subtitle.filter(|s| !s.is_empty());
        let empty = title.is_none() && subtitle.is_none() && body.is_empty();

        let mut out = Vec::new();
        match title {
            Some(content) => out.push(Block::Heading { level: 1, content }),
            None => out.push(Block::heading(1, format!("Slide {number}"))),
        }
        if let Some(content) = subtitle {
            out.push(Block::Heading { level: 2, content });
        }
        if empty {
            out.push(Block::emphasized("Empty slide"));
        }
        out.extend(body);
        if !notes.is_empty() {
            let mut line = vec![
                Inline::Strong(vec![Inline::Text("Notes".to_string())]),
                Inline::Text(": ".to_string()),
            ];
            for (i, mut paragraph) in notes.into_iter().enumerate() {
                if i > 0 {
                    line.push(Inline::Break);
                }
                line.append(&mut paragraph);
            }
            out.push(Block::Quote(vec![Block::Paragraph(line)]));
        }
        out
    }

    /// The paragraphs of a title shape on one line.
    fn joined_paragraphs(&mut self, shape: &Elem) -> Vec<Inline> {
        let mut out = Vec::new();
        for (i, mut paragraph) in self.paragraphs_of(shape).into_iter().enumerate() {
            if i > 0 {
                out.push(Inline::Text(" ".to_string()));
            }
            out.append(&mut paragraph);
        }
        out
    }
}

/// `table:number-*-repeated`, at least 1.
fn repeat_of(e: &Elem, name: &str) -> usize {
    e.attr(name)
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

/// OpenDocument reads any run of white space in text as one space.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_ascii_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn read_entry(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    format: &'static str,
) -> Result<String> {
    let mut file = archive
        .by_name(name)
        .map_err(|e| err(format, format!("Entry not found: {name}: {e}")))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:presentation="urn:oasis:names:tc:opendocument:xmlns:presentation:1.0" xmlns:style="urn:oasis:names:tc:opendocument:xmlns:style:1.0" xmlns:fo="urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:svg="urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0" xmlns:dc="http://purl.org/dc/elements/1.1/""#;

    const STYLES: &str = r#"<office:automatic-styles>
        <style:style style:name="B" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style>
        <style:style style:name="I" style:family="text"><style:text-properties fo:font-style="italic"/></style:style>
        <style:style style:name="BI" style:family="text" style:parent-style-name="B"><style:text-properties fo:font-style="italic"/></style:style>
        <style:style style:name="S" style:family="text"><style:text-properties style:text-line-through-style="solid"/></style:style>
        <text:list-style style:name="Num"><text:list-level-style-number text:level="1"/><text:list-level-style-bullet text:level="2"/></text:list-style>
        <text:list-style style:name="Bul"><text:list-level-style-bullet text:level="1"/></text:list-style>
    </office:automatic-styles>"#;

    fn make_odf(mime: &str, content: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", opts).unwrap();
        zip.write_all(mime.as_bytes()).unwrap();
        zip.start_file("content.xml", opts).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
        for (name, data) in extra {
            zip.start_file(*name, opts).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn odt(body: &str) -> Vec<u8> {
        make_odf(
            "application/vnd.oasis.opendocument.text",
            &format!(
                r#"<office:document-content {NS}>{STYLES}<office:body><office:text>{body}</office:text></office:body></office:document-content>"#
            ),
            &[],
        )
    }

    fn odp(slides: &str) -> Vec<u8> {
        make_odf(
            "application/vnd.oasis.opendocument.presentation",
            &format!(
                r#"<office:document-content {NS}>{STYLES}<office:body><office:presentation>{slides}</office:presentation></office:body></office:document-content>"#
            ),
            &[],
        )
    }

    fn to_md(converter: &dyn Converter, bytes: &[u8]) -> String {
        let mut out = Vec::new();
        converter.convert(bytes, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn headings_paragraphs_and_marks() {
        let md = to_md(
            &OdtConverter,
            &odt(r#"<text:h text:outline-level="2">Intro</text:h>
                <text:p>Plain <text:span text:style-name="B">bold</text:span>, <text:span text:style-name="I">italic</text:span>,
                <text:span text:style-name="BI">both</text:span> and <text:span text:style-name="S">gone</text:span>.</text:p>
                <text:p/>"#),
        );
        assert_eq!(
            md,
            "## Intro\n\nPlain **bold**, *italic*, ***both*** and ~~gone~~.\n"
        );
    }

    #[test]
    fn links_spaces_breaks_and_entities() {
        let md = to_md(
            &OdtConverter,
            &odt(
                r#"<text:p>Tom &amp; Jerry<text:s text:c="2"/>x<text:line-break/>next
                <text:a xlink:href="https://example.com/?a=1&amp;b=2">site</text:a></text:p>"#,
            ),
        );
        assert_eq!(
            md,
            "Tom & Jerry  x  \nnext [site](https://example.com/?a=1&b=2)\n"
        );
    }

    #[test]
    fn lists_follow_their_list_style_and_nest() {
        let md = to_md(
            &OdtConverter,
            &odt(r#"<text:list text:style-name="Num">
                    <text:list-item><text:p>one</text:p>
                        <text:list><text:list-item><text:p>inner</text:p></text:list-item></text:list>
                    </text:list-item>
                    <text:list-item><text:p>two</text:p></text:list-item>
                </text:list>
                <text:list text:style-name="Bul"><text:list-item><text:p>dot</text:p></text:list-item></text:list>"#),
        );
        assert_eq!(md, "1. one\n   - inner\n2. two\n\n- dot\n");
    }

    #[test]
    fn tables_expand_repeats_and_drop_trailing_empty_cells() {
        let md = to_md(
            &OdtConverter,
            &odt(r#"<table:table><table:table-header-rows><table:table-row>
                    <table:table-cell><text:p>A</text:p></table:table-cell>
                    <table:table-cell><text:p>B</text:p></table:table-cell>
                    <table:table-cell table:number-columns-repeated="1000"/>
                </table:table-row></table:table-header-rows>
                <table:table-row>
                    <table:table-cell><text:p>1</text:p></table:table-cell>
                    <table:covered-table-cell/>
                </table:table-row></table:table>"#),
        );
        assert_eq!(md, "| A | B |\n| - | - |\n| 1 |   |\n");
    }

    #[test]
    fn footnotes_and_comments_are_defined_at_the_end() {
        let md = to_md(
            &OdtConverter,
            &odt(
                r#"<text:p>Claim<text:note text:note-class="footnote"><text:note-citation>1</text:note-citation>
                    <text:note-body><text:p>The source.</text:p></text:note-body></text:note>
                    and<office:annotation><dc:creator>Alice</dc:creator><dc:date>2024-05-06T07:08:09</dc:date><text:p>Check this</text:p></office:annotation> more.</text:p>"#,
            ),
        );
        assert_eq!(
            md,
            "Claim[^1] and[^c1] more.\n\n[^1]: The source.\n\n[^c1]: **Alice** (2024-05-06): Check this\n"
        );
    }

    #[test]
    fn quotations_and_preformatted_paragraphs() {
        let md = to_md(
            &OdtConverter,
            &odt(r#"<text:p text:style-name="Quotations">Wise words</text:p>
                <text:p text:style-name="Preformatted_20_Text">let x = 1;</text:p>"#),
        );
        assert_eq!(md, "> Wise words\n\n```\nlet x = 1;\n```\n");
    }

    #[test]
    fn generated_indexes_are_skipped() {
        let md = to_md(
            &OdtConverter,
            &odt(
                r#"<text:table-of-content><text:index-body><text:p>Contents 1</text:p></text:index-body></text:table-of-content>
                <text:section><text:p>Body</text:p></text:section>"#,
            ),
        );
        assert_eq!(md, "Body\n");
    }

    #[test]
    fn images_are_saved_only_when_asked() {
        let body = r#"<text:p>Before<draw:frame><draw:image xlink:href="Pictures/a.png"/><svg:title>A chart</svg:title></draw:frame></text:p>"#;
        let bytes = make_odf(
            "application/vnd.oasis.opendocument.text",
            &format!(
                r#"<office:document-content {NS}><office:body><office:text>{body}</office:text></office:body></office:document-content>"#
            ),
            &[("Pictures/a.png", b"PNGDATA")],
        );
        assert_eq!(to_md(&OdtConverter, &bytes), "Before\n");

        let dir = std::env::temp_dir().join(format!("mq-conv-odt-{}", std::process::id()));
        let options = ConvertOptions {
            image_dir: Some(dir.clone()),
            ..Default::default()
        };
        let mut out = Vec::new();
        OdtConverter
            .convert_with(&bytes, &mut out, &options)
            .unwrap();
        let md = String::from_utf8(out).unwrap();
        let link = format!("![A chart]({})", dir.join("a.png").to_string_lossy());
        assert!(md.contains(&link), "{md}");
        assert_eq!(std::fs::read(dir.join("a.png")).unwrap(), b"PNGDATA");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejects_files_that_are_not_odt() {
        let mut out = Vec::new();
        assert!(OdtConverter.convert(b"not a zip", &mut out).is_err());
        let no_body = make_odf(
            "application/vnd.oasis.opendocument.text",
            &format!(r#"<office:document-content {NS}/>"#),
            &[],
        );
        assert!(OdtConverter.convert(&no_body, &mut out).is_err());
    }

    fn slide(title: Option<&str>, body: &str, notes: Option<&str>) -> String {
        let title = title.map_or(String::new(), |t| {
            format!(
                r#"<draw:frame presentation:class="title"><draw:text-box><text:p>{t}</text:p></draw:text-box></draw:frame>"#
            )
        });
        let notes = notes.map_or(String::new(), |n| {
            format!(
                r#"<presentation:notes><draw:frame presentation:class="page"/><draw:frame presentation:class="notes"><draw:text-box><text:p>{n}</text:p></draw:text-box></draw:frame></presentation:notes>"#
            )
        });
        format!(r#"<draw:page draw:name="p">{title}{body}{notes}</draw:page>"#)
    }

    const OUTLINE: &str = r#"<draw:frame presentation:class="outline"><draw:text-box><text:list text:style-name="Bul"><text:list-item><text:p>point</text:p></text:list-item></text:list></draw:text-box></draw:frame>"#;

    #[test]
    fn slides_have_titles_lists_and_notes() {
        let deck = format!(
            "{}{}{}",
            slide(Some("First"), OUTLINE, Some("Say hi")),
            slide(None, "", None),
            slide(Some("Third"), "", None)
        );
        let md = to_md(&OdpConverter, &odp(&deck));
        assert_eq!(
            md,
            "# First\n\n- point\n\n> **Notes**: Say hi\n\n---\n\n# Slide 2\n\n*Empty slide*\n\n---\n\n# Third\n"
        );
    }

    #[test]
    fn slide_selection_keeps_numbers_and_rejects_empty_matches() {
        let deck = format!(
            "{}{}{}",
            slide(None, OUTLINE, None),
            slide(None, OUTLINE, None),
            slide(None, OUTLINE, None)
        );
        let bytes = odp(&deck);
        let options = ConvertOptions {
            pages: Some("2".parse().unwrap()),
            ..Default::default()
        };
        let mut out = Vec::new();
        OdpConverter
            .convert_with(&bytes, &mut out, &options)
            .unwrap();
        assert!(String::from_utf8(out).unwrap().starts_with("# Slide 2\n"));

        let options = ConvertOptions {
            pages: Some("9".parse().unwrap()),
            ..Default::default()
        };
        assert!(
            OdpConverter
                .convert_with(&bytes, &mut Vec::new(), &options)
                .is_err()
        );
    }

    #[test]
    fn named_styles_from_styles_xml_apply() {
        let content = format!(
            r#"<office:document-content {NS}><office:body><office:text><text:p><text:span text:style-name="Strong_20_Emphasis">key</text:span></text:p></office:text></office:body></office:document-content>"#
        );
        let styles = format!(
            r#"<office:document-styles {NS}><office:styles><style:style style:name="Strong_20_Emphasis" style:family="text"><style:text-properties fo:font-weight="bold"/></style:style></office:styles></office:document-styles>"#
        );
        let bytes = make_odf(
            "application/vnd.oasis.opendocument.text",
            &content,
            &[("styles.xml", styles.as_bytes())],
        );
        assert_eq!(to_md(&OdtConverter, &bytes), "**key**\n");
    }
}
