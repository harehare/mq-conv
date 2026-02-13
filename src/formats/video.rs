use std::io::{Cursor, Write};

use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::ItemKey;

use crate::converter::Converter;
use crate::error::{Error, Result};

pub struct VideoConverter;

impl Converter for VideoConverter {
    fn format_name(&self) -> &'static str {
        "video"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let cursor = Cursor::new(input);
        let tagged_file =
            Probe::new(cursor)
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

        writeln!(writer, "# Video")?;
        writeln!(writer)?;

        let props = tagged_file.properties();
        writeln!(writer, "## File Info")?;
        writeln!(writer)?;
        writeln!(writer, "| Property | Value |")?;
        writeln!(writer, "|----------|-------|")?;

        writeln!(
            writer,
            "| Format | {:?} |",
            tagged_file.file_type()
        )?;
        writeln!(writer, "| Size | {} |", format_size(input.len() as u64))?;

        let duration = props.duration();
        if !duration.is_zero() {
            let total_secs = duration.as_secs();
            let hours = total_secs / 3600;
            let mins = (total_secs % 3600) / 60;
            let secs = total_secs % 60;
            if hours > 0 {
                writeln!(writer, "| Duration | {hours}:{mins:02}:{secs:02} |")?;
            } else {
                writeln!(writer, "| Duration | {mins}:{secs:02} |")?;
            }
        }

        if let Some(bitrate) = props.overall_bitrate() {
            writeln!(writer, "| Bitrate | {bitrate} kbps |")?;
        }

        if let Some(channels) = props.channels() {
            let ch_label = match channels {
                1 => "Mono",
                2 => "Stereo",
                6 => "5.1 Surround",
                8 => "7.1 Surround",
                _ => "Multi-channel",
            };
            writeln!(writer, "| Audio Channels | {channels} ({ch_label}) |")?;
        }

        if let Some(sample_rate) = props.sample_rate() {
            writeln!(writer, "| Audio Sample Rate | {sample_rate} Hz |")?;
        }

        writeln!(writer)?;

        // Tags
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
                writeln!(writer, "## Tags")?;
                writeln!(writer)?;
                writeln!(writer, "| Tag | Value |")?;
                writeln!(writer, "|-----|-------|")?;
                for (key, value) in &items {
                    writeln!(writer, "| {key} | {} |", value.replace('|', "\\|"))?;
                }
            }
        }

        Ok(())
    }
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}
