//! Embedded image extraction (JPEG passthrough, raw pixels re-encoded as PNG).

use std::io::Write;

use lopdf::{Document, Object, ObjectId, Stream};

use super::util::{dict_get, num, resolve};

pub struct ExtractedImage {
    pub data: Vec<u8>,
    pub ext: &'static str,
}

fn filters(doc: &Document, stream: &Stream) -> Vec<String> {
    match dict_get(doc, &stream.dict, b"Filter") {
        Some(Object::Name(n)) => vec![String::from_utf8_lossy(n).into_owned()],
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(|o| resolve(doc, o).as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect(),
        _ => Vec::new(),
    }
}

/// Number of colour components and (for Indexed) a palette.
struct ColorInfo {
    comps: usize,
    palette: Option<(Vec<u8>, usize)>,
    cmyk: bool,
}

fn color_info(doc: &Document, cs: Option<&Object>) -> ColorInfo {
    let simple = |comps: usize, cmyk: bool| ColorInfo {
        comps,
        palette: None,
        cmyk,
    };
    match cs {
        Some(Object::Name(n)) => match n.as_slice() {
            b"DeviceGray" | b"G" | b"CalGray" => simple(1, false),
            b"DeviceCMYK" | b"CMYK" => simple(4, true),
            _ => simple(3, false),
        },
        Some(Object::Array(a)) if !a.is_empty() => {
            match resolve(doc, &a[0]).as_name().unwrap_or(b"") {
                b"ICCBased" => {
                    let n = a
                        .get(1)
                        .and_then(|o| resolve(doc, o).as_stream().ok())
                        .and_then(|s| dict_get(doc, &s.dict, b"N"))
                        .and_then(num)
                        .unwrap_or(3.0) as usize;
                    simple(n, n == 4)
                }
                b"CalGray" => simple(1, false),
                b"Indexed" | b"I" => {
                    let base = color_info(doc, a.get(1).map(|o| resolve(doc, o)));
                    let lookup = match a.get(3).map(|o| resolve(doc, o)) {
                        Some(Object::String(s, _)) => s.clone(),
                        Some(Object::Stream(s)) => s.decompressed_content().unwrap_or_default(),
                        _ => Vec::new(),
                    };
                    ColorInfo {
                        comps: 1,
                        palette: Some((lookup, base.comps)),
                        cmyk: base.cmyk,
                    }
                }
                _ => simple(3, false),
            }
        }
        _ => simple(1, false),
    }
}

pub fn extract(doc: &Document, id: ObjectId) -> Option<ExtractedImage> {
    let stream = doc.get_object(id).ok()?.as_stream().ok()?;
    let fl = filters(doc, stream);
    match fl.last().map(String::as_str) {
        Some("DCTDecode") | Some("DCT") if fl.len() == 1 => {
            return Some(ExtractedImage {
                data: stream.content.clone(),
                ext: "jpg",
            });
        }
        Some("JPXDecode") if fl.len() == 1 => {
            return Some(ExtractedImage {
                data: stream.content.clone(),
                ext: "jp2",
            });
        }
        Some("CCITTFaxDecode") | Some("JBIG2Decode") | Some("DCTDecode") | Some("JPXDecode") => {
            return None;
        }
        _ => {}
    }

    let w = dict_get(doc, &stream.dict, b"Width").and_then(num)? as usize;
    let h = dict_get(doc, &stream.dict, b"Height").and_then(num)? as usize;
    if w < 16 || h < 16 || w * h > 100_000_000 {
        return None;
    }
    let is_mask = matches!(
        dict_get(doc, &stream.dict, b"ImageMask"),
        Some(Object::Boolean(true))
    );
    let bpc = if is_mask {
        1
    } else {
        dict_get(doc, &stream.dict, b"BitsPerComponent")
            .and_then(num)
            .unwrap_or(8.0) as usize
    };
    let info = if is_mask {
        ColorInfo {
            comps: 1,
            palette: None,
            cmyk: false,
        }
    } else {
        color_info(
            doc,
            stream.dict.get(b"ColorSpace").ok().map(|o| resolve(doc, o)),
        )
    };
    let raw = stream.decompressed_content().ok()?;
    let row_bytes = (w * info.comps * bpc).div_ceil(8);
    if raw.len() < row_bytes * h || !matches!(bpc, 1 | 2 | 4 | 8) {
        return None;
    }

    // Unpack to 8-bit samples, then to Gray or RGB.
    let max = ((1u32 << bpc) - 1) as f32;
    let sample = |row: &[u8], k: usize| -> u8 {
        match bpc {
            8 => row[k],
            _ => {
                let bit = k * bpc;
                let v = (row[bit / 8] >> (8 - bpc - bit % 8)) & ((1 << bpc) - 1);
                if info.palette.is_some() {
                    v
                } else {
                    (v as f32 / max * 255.0) as u8
                }
            }
        }
    };
    let rgb_out = info.comps >= 3 || info.palette.as_ref().is_some_and(|(_, n)| *n >= 3);
    let mut pixels: Vec<u8> = Vec::with_capacity(w * h * if rgb_out { 3 } else { 1 });
    for y in 0..h {
        let row = &raw[y * row_bytes..(y + 1) * row_bytes];
        for x in 0..w {
            if let Some((lookup, n)) = &info.palette {
                let idx = sample(row, x) as usize;
                let entry = lookup.get(idx * n..idx * n + n).unwrap_or(&[]);
                match (n, entry) {
                    (1, [g]) => pixels.push(*g),
                    (3, [r, g, b]) => pixels.extend([*r, *g, *b]),
                    (4, [c, m, yy, k]) => pixels.extend(cmyk_to_rgb(*c, *m, *yy, *k)),
                    _ => pixels.extend(if rgb_out { vec![0, 0, 0] } else { vec![0] }),
                }
            } else if info.comps == 1 {
                pixels.push(sample(row, x));
            } else if info.comps == 3 {
                pixels.extend([
                    sample(row, x * 3),
                    sample(row, x * 3 + 1),
                    sample(row, x * 3 + 2),
                ]);
            } else if info.cmyk {
                pixels.extend(cmyk_to_rgb(
                    sample(row, x * 4),
                    sample(row, x * 4 + 1),
                    sample(row, x * 4 + 2),
                    sample(row, x * 4 + 3),
                ));
            } else {
                return None;
            }
        }
    }
    let data = encode_png(w as u32, h as u32, if rgb_out { 2 } else { 0 }, &pixels)?;
    Some(ExtractedImage { data, ext: "png" })
}

fn cmyk_to_rgb(c: u8, m: u8, y: u8, k: u8) -> [u8; 3] {
    let f = |v: u8| ((255 - v as u32) * (255 - k as u32) / 255) as u8;
    [f(c), f(m), f(y)]
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend((data.len() as u32).to_be_bytes());
    let mut body = kind.to_vec();
    body.extend(data);
    out.extend(&body);
    out.extend(crc32(&body).to_be_bytes());
}

fn encode_png(w: u32, h: u32, color_type: u8, pixels: &[u8]) -> Option<Vec<u8>> {
    let channels = if color_type == 2 { 3 } else { 1 };
    let row = w as usize * channels;
    let mut scan = Vec::with_capacity((row + 1) * h as usize);
    for y in 0..h as usize {
        scan.push(0);
        scan.extend(pixels.get(y * row..(y + 1) * row)?);
    }
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    enc.write_all(&scan).ok()?;
    let z = enc.finish().ok()?;

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, color_type, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &z);
    chunk(&mut out, b"IEND", &[]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_reference_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn png_has_signature_and_valid_chunks() {
        let png = encode_png(2, 2, 2, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9]).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // Walk the chunks and verify each CRC.
        let mut pos = 8;
        let mut kinds = Vec::new();
        while pos < png.len() {
            let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
            let body = &png[pos + 4..pos + 8 + len];
            let crc = u32::from_be_bytes(png[pos + 8 + len..pos + 12 + len].try_into().unwrap());
            assert_eq!(crc32(body), crc);
            kinds.push(String::from_utf8_lossy(&body[..4]).into_owned());
            pos += 12 + len;
        }
        assert_eq!(kinds, ["IHDR", "IDAT", "IEND"]);
    }

    #[test]
    fn short_pixel_data_is_rejected() {
        assert!(encode_png(4, 4, 0, &[0; 3]).is_none());
    }

    #[test]
    fn cmyk_black_and_white() {
        assert_eq!(cmyk_to_rgb(0, 0, 0, 0), [255, 255, 255]);
        assert_eq!(cmyk_to_rgb(0, 0, 0, 255), [0, 0, 0]);
    }
}
