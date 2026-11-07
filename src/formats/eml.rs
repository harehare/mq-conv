//! E-mail messages (`.eml`) → Markdown.
//!
//! The headers become a list under the subject, the text body follows with
//! quoted replies as block quotes, and attachments are listed by name.

use std::io::Write;

use mail_parser::{Address, AddressList, Message, MessageParser};

use crate::converter::Converter;
use crate::document::{Block, Document, Inline};
use crate::error::{Error, Result};
use crate::formats::media::format_size;

pub struct EmlConverter;

impl Converter for EmlConverter {
    fn format_name(&self) -> &'static str {
        "eml"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let message = MessageParser::default()
            .parse(input)
            .filter(has_mail_headers)
            .ok_or_else(|| Error::Conversion {
                format: "eml",
                message: "not an e-mail message".to_string(),
            })?;

        let mut doc = Document::new();
        doc.push(Block::heading(
            1,
            message.subject().map_or("(no subject)", str::trim),
        ));

        let fields = header_fields(&message);
        if !fields.is_empty() {
            doc.push(Block::List {
                ordered: false,
                start: 1,
                items: fields
                    .into_iter()
                    .map(|(label, mut value)| {
                        let mut line = vec![
                            Inline::Strong(vec![Inline::Text(label.to_string())]),
                            Inline::Text(": ".to_string()),
                        ];
                        line.append(&mut value);
                        vec![Block::Paragraph(line)]
                    })
                    .collect(),
            });
        }
        doc.push(Block::Rule);

        let body: Vec<String> = message.text_bodies().map(|b| b.into_owned()).collect();
        doc.extend(body_blocks(&body.join("\n\n")));

        let attachments = attachment_items(&message);
        if !attachments.is_empty() {
            doc.push(Block::heading(2, "Attachments"));
            doc.push(Block::List {
                ordered: false,
                start: 1,
                items: attachments,
            });
        }
        doc.write_markdown(writer)
    }
}

/// A parse of arbitrary bytes still yields a message; one without any of the
/// usual headers is not mail.
fn has_mail_headers(message: &Message) -> bool {
    message.subject().is_some()
        || message.from().is_some()
        || message.to().is_some()
        || message.date().is_some()
}

fn header_fields(message: &Message) -> Vec<(&'static str, Vec<Inline>)> {
    let mut fields = Vec::new();
    for (label, list) in [
        ("From", message.from()),
        ("To", message.to()),
        ("Cc", message.cc()),
        ("Bcc", message.bcc()),
        ("Reply-To", message.reply_to()),
    ] {
        if let Some(inlines) = list.map(addresses).filter(|i| !i.is_empty()) {
            fields.push((label, inlines));
        }
    }
    if let Some(date) = message.date() {
        fields.push(("Date", vec![Inline::Text(date.to_rfc3339())]));
    }
    fields
}

/// `[Name](mailto:address)` for each mailbox, groups flattened.
fn addresses(list: AddressList) -> Vec<Inline> {
    let mut out = Vec::new();
    let mut push = |name: Option<&str>, address: Option<&str>| {
        let name = name.map(str::trim).filter(|n| !n.is_empty());
        let address = address.map(str::trim).filter(|a| !a.is_empty());
        let inline = match (name, address) {
            (Some(name), Some(address)) => Inline::Link {
                url: format!("mailto:{address}"),
                content: vec![Inline::Text(name.to_string())],
            },
            (None, Some(address)) => Inline::Link {
                url: format!("mailto:{address}"),
                content: vec![Inline::Text(address.to_string())],
            },
            (Some(name), None) => Inline::Text(name.to_string()),
            (None, None) => return,
        };
        if !out.is_empty() {
            out.push(Inline::Text(", ".to_string()));
        }
        out.push(inline);
    };
    for entry in list.iter() {
        match entry {
            Address::Mailbox(m) => push(m.name(), m.address()),
            Address::Group(g) => {
                for m in g.mailboxes() {
                    push(m.name(), m.address());
                }
            }
        }
    }
    out
}

fn attachment_items(message: &Message) -> Vec<Vec<Block>> {
    message
        .attachments()
        .map(|part| {
            let name = part.attachment_name().unwrap_or("(unnamed)");
            let kind = part
                .content_type()
                .map(|t| match t.subtype() {
                    Some(sub) => format!("{}/{sub}", t.ctype()),
                    None => t.ctype().to_string(),
                })
                .unwrap_or_default();
            let size = format_size(part.decoded_len() as u64);
            let detail = if kind.is_empty() {
                size
            } else {
                format!("{kind}, {size}")
            };
            vec![Block::Paragraph(vec![
                Inline::Code(name.to_string()),
                Inline::Text(format!(" ({detail})")),
            ])]
        })
        .collect()
}

/// Plain text as paragraphs, keeping the author's line breaks. Lines quoted
/// with `>` become block quotes, nested by depth.
fn body_blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    quote_blocks(&lines)
}

fn quote_blocks(lines: &[&str]) -> Vec<Block> {
    let is_quoted = |l: &&str| l.trim_start().starts_with('>');
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let quoted = is_quoted(&lines[i]);
        let end = lines[i..]
            .iter()
            .position(|l| is_quoted(l) != quoted)
            .map_or(lines.len(), |n| i + n);
        let run = &lines[i..end];
        if quoted {
            let inner: Vec<&str> = run
                .iter()
                .map(|l| {
                    let rest = l.trim_start().strip_prefix('>').unwrap_or(l);
                    rest.strip_prefix(' ').unwrap_or(rest)
                })
                .collect();
            blocks.push(Block::Quote(quote_blocks(&inner)));
        } else {
            blocks.extend(paragraphs(run));
        }
        i = end;
    }
    blocks
}

/// Blank lines separate paragraphs; line ends inside one stay hard breaks.
fn paragraphs(lines: &[&str]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current: Vec<Inline> = Vec::new();
    for line in lines {
        let line = line.trim_end();
        if line.is_empty() {
            if !current.is_empty() {
                blocks.push(Block::Paragraph(std::mem::take(&mut current)));
            }
            continue;
        }
        if !current.is_empty() {
            current.push(Inline::Break);
        }
        current.push(Inline::Text(line.to_string()));
    }
    if !current.is_empty() {
        blocks.push(Block::Paragraph(current));
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(raw: &str) -> String {
        let mut out = Vec::new();
        EmlConverter.convert(raw.as_bytes(), &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn headers_become_a_list_and_the_body_follows() {
        let md = convert(
            "From: Alice <alice@example.com>\r\n\
             To: bob@example.com, \"Carol C\" <carol@example.com>\r\n\
             Date: Sat, 20 Nov 2021 14:22:01 -0800\r\n\
             Subject: Plans\r\n\
             \r\n\
             Hello Bob,\r\n\
             see you soon.\r\n\
             \r\n\
             Alice\r\n",
        );
        assert_eq!(
            md,
            "# Plans\n\n\
             - **From**: [Alice](mailto:alice@example.com)\n\
             - **To**: [bob@example.com](mailto:bob@example.com), [Carol C](mailto:carol@example.com)\n\
             - **Date**: 2021-11-20T14:22:01-08:00\n\n\
             ---\n\n\
             Hello Bob,  \nsee you soon.\n\n\
             Alice\n"
        );
    }

    #[test]
    fn encoded_headers_and_japanese_bodies_are_decoded() {
        let md = convert(
            "From: =?UTF-8?B?5bGx55Sw?= <yamada@example.jp>\r\n\
             Subject: =?UTF-8?B?44GT44KT44Gr44Gh44Gv?=\r\n\
             Content-Type: text/plain; charset=UTF-8\r\n\
             Content-Transfer-Encoding: base64\r\n\
             \r\n\
             5pys5paH44Gn44GZ\r\n",
        );
        assert!(md.starts_with("# こんにちは\n"), "{md}");
        assert!(md.contains("[山田](mailto:yamada@example.jp)"), "{md}");
        assert!(md.contains("本文です"), "{md}");
    }

    #[test]
    fn iso_2022_jp_bodies_are_decoded() {
        // 日本語 in ISO-2022-JP.
        let raw = "From: a@example.jp\r\nSubject: s\r\n\
                   Content-Type: text/plain; charset=ISO-2022-JP\r\n\
                   Content-Transfer-Encoding: 7bit\r\n\r\n\
                   \u{1b}$BF|K\\8l\u{1b}(B\r\n";
        assert!(convert(raw).contains("日本語"));
    }

    #[test]
    fn quoted_replies_become_nested_block_quotes() {
        let md = convert(
            "From: a@example.com\r\nSubject: Re: x\r\n\r\n\
             Sounds good.\r\n\
             > Can we meet?\r\n\
             >> Earlier question\r\n\
             \r\n\
             Thanks\r\n",
        );
        assert!(
            md.contains("Sounds good.\n\n> Can we meet?\n>\n> > Earlier question\n\nThanks\n"),
            "{md}"
        );
    }

    #[test]
    fn html_only_mail_reads_as_text() {
        let md = convert(
            "From: a@example.com\r\nSubject: News\r\n\
             Content-Type: text/html; charset=UTF-8\r\n\r\n\
             <html><body><p>Big <b>news</b> today</p></body></html>\r\n",
        );
        assert!(md.contains("Big news today"), "{md}");
        assert!(!md.contains("<b>"), "{md}");
    }

    #[test]
    fn attachments_are_listed_with_type_and_size() {
        let md = convert(
            "From: a@example.com\r\nSubject: Files\r\n\
             Content-Type: multipart/mixed; boundary=\"b\"\r\n\r\n\
             --b\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n\
             --b\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\n\
             Content-Disposition: attachment; filename=\"report.pdf\"\r\n\
             Content-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQK\r\n\
             --b--\r\n",
        );
        assert!(md.contains("See attached."), "{md}");
        assert!(
            md.contains("## Attachments\n\n- `report.pdf` (application/pdf, 9 B)\n"),
            "{md}"
        );
    }

    #[test]
    fn input_without_mail_headers_is_rejected() {
        let mut out = Vec::new();
        assert!(EmlConverter.convert(b"just some words", &mut out).is_err());
    }
}
