use std::io::{Cursor, Write};

use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;

use crate::converter::Converter;
use crate::document::{Block, Document};
use crate::error::{Error, Result};
use crate::formats::media::format_size;

pub struct VideoConverter;

impl Converter for VideoConverter {
    fn format_name(&self) -> &'static str {
        "video"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let cursor = Cursor::new(input);
        let tagged_file = Probe::new(cursor)
            .guess_file_type()
            .map_err(|e| Error::Conversion {
                format: "video",
                message: e.to_string(),
            })?
            .read()
            .map_err(|e| Error::Conversion {
                format: "video",
                message: e.to_string(),
            })?;

        let mut doc = Document::new();
        doc.push(Block::heading(1, "Video"));

        let props = tagged_file.properties();
        doc.push(Block::heading(2, "File Info"));
        let mut rows = vec![
            ("Format", format!("{:?}", tagged_file.file_type())),
            ("Size", format_size(input.len() as u64)),
        ];

        let duration = props.duration();
        if !duration.is_zero() {
            let total = duration.as_secs();
            let (hours, mins, secs) = (total / 3600, (total % 3600) / 60, total % 60);
            let value = if hours > 0 {
                format!("{hours}:{mins:02}:{secs:02}")
            } else {
                format!("{mins}:{secs:02}")
            };
            rows.push(("Duration", value));
        }
        if let Some(bitrate) = props.overall_bitrate() {
            rows.push(("Bitrate", format!("{bitrate} kbps")));
        }
        if let Some(channels) = props.channels() {
            let label = match channels {
                1 => "Mono",
                2 => "Stereo",
                6 => "5.1 Surround",
                8 => "7.1 Surround",
                _ => "Multi-channel",
            };
            rows.push(("Audio Channels", format!("{channels} ({label})")));
        }
        if let Some(sample_rate) = props.sample_rate() {
            rows.push(("Audio Sample Rate", format!("{sample_rate} Hz")));
        }
        doc.push(Block::key_value_table(["Property", "Value"], &rows));

        if let Some(tag) = tagged_file.primary_tag().or(tagged_file.first_tag()) {
            let items: Vec<(&str, String)> = [
                ("Title", tag.get_string(ItemKey::TrackTitle)),
                ("Artist", tag.get_string(ItemKey::TrackArtist)),
                ("Album", tag.get_string(ItemKey::AlbumTitle)),
                ("Year", tag.get_string(ItemKey::Year)),
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
