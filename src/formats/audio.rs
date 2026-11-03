use std::io::{Cursor, Write};

use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;

use crate::converter::Converter;
use crate::document::{Block, Document};
use crate::error::{Error, Result};
use crate::formats::media::format_size;

pub struct AudioConverter;

impl Converter for AudioConverter {
    fn format_name(&self) -> &'static str {
        "audio"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let cursor = Cursor::new(input);
        let tagged_file = Probe::new(cursor)
            .guess_file_type()
            .map_err(|e| Error::Conversion {
                format: "audio",
                message: e.to_string(),
            })?
            .read()
            .map_err(|e| Error::Conversion {
                format: "audio",
                message: e.to_string(),
            })?;

        let mut doc = Document::new();
        doc.push(Block::heading(1, "Audio"));

        let props = tagged_file.properties();
        doc.push(Block::heading(2, "File Info"));
        let mut rows = vec![
            ("Format", format!("{:?}", tagged_file.file_type())),
            ("Size", format_size(input.len() as u64)),
        ];

        let duration = props.duration();
        if !duration.is_zero() {
            let secs = duration.as_secs();
            rows.push(("Duration", format!("{}:{:02}", secs / 60, secs % 60)));
        }
        if let Some(bitrate) = props.overall_bitrate() {
            rows.push(("Bitrate", format!("{bitrate} kbps")));
        }
        if let Some(sample_rate) = props.sample_rate() {
            rows.push(("Sample Rate", format!("{sample_rate} Hz")));
        }
        if let Some(channels) = props.channels() {
            let label = match channels {
                1 => "Mono",
                2 => "Stereo",
                _ => "Multi-channel",
            };
            rows.push(("Channels", format!("{channels} ({label})")));
        }
        doc.push(Block::key_value_table(["Property", "Value"], &rows));

        if let Some(tag) = tagged_file.primary_tag().or(tagged_file.first_tag()) {
            let items: Vec<(&str, String)> = [
                ("Title", tag.get_string(ItemKey::TrackTitle)),
                ("Artist", tag.get_string(ItemKey::TrackArtist)),
                ("Album", tag.get_string(ItemKey::AlbumTitle)),
                ("Year", tag.get_string(ItemKey::Year)),
                ("Track", tag.get_string(ItemKey::TrackNumber)),
                ("Genre", tag.get_string(ItemKey::Genre)),
                ("Comment", tag.get_string(ItemKey::Comment)),
            ]
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, v.to_string())))
            .collect();

            if !items.is_empty() {
                doc.push(Block::heading(2, "Tags"));
                doc.push(Block::key_value_table(["Tag", "Value"], &items));
            }
        }

        doc.write_markdown(writer)
    }
}
