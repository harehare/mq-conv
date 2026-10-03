//! PDF font handling: code → Unicode decoding and glyph advance widths.

use std::collections::HashMap;

use lopdf::{Dictionary, Document, Object};

use super::util::{dict_get, num, resolve};

/// One decoded character code from a text-showing string.
pub struct DecodedGlyph {
    pub text: String,
    /// Advance width in 1/1000 text-space units.
    pub width: f32,
    /// Single-byte code 32, which `Tw` word spacing applies to.
    pub is_space: bool,
}

#[derive(Default)]
struct CMap {
    code_lengths: Vec<usize>,
    chars: HashMap<u32, String>,
    ranges: Vec<(u32, u32, Vec<u16>)>,
}

impl CMap {
    fn lookup(&self, code: u32) -> Option<String> {
        if let Some(s) = self.chars.get(&code) {
            return Some(s.clone());
        }
        for (lo, hi, dst) in &self.ranges {
            if code >= *lo && code <= *hi {
                let mut units = dst.clone();
                if let Some(last) = units.last_mut() {
                    *last = last.wrapping_add((code - lo) as u16);
                }
                return Some(String::from_utf16_lossy(&units));
            }
        }
        None
    }
}

fn hex_to_u32(h: &str) -> u32 {
    u32::from_str_radix(h, 16).unwrap_or(0)
}

fn hex_to_utf16(h: &str) -> Vec<u16> {
    let bytes: Vec<u8> = (0..h.len() / 2 * 2)
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&h[i..i + 2], 16).ok())
        .collect();
    if bytes.len() == 1 {
        return vec![bytes[0] as u16];
    }
    bytes
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect()
}

/// Minimal ToUnicode CMap parser (codespacerange / bfchar / bfrange).
fn parse_cmap(data: &[u8]) -> CMap {
    let text = String::from_utf8_lossy(data);
    let mut tokens: Vec<String> = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '<' {
            chars.next();
            if chars.peek() == Some(&'<') {
                chars.next();
                tokens.push("<<".into());
                continue;
            }
            let mut h = String::from("<");
            for c in chars.by_ref() {
                if c == '>' {
                    break;
                }
                if !c.is_whitespace() {
                    h.push(c);
                }
            }
            tokens.push(h);
        } else if c == '[' || c == ']' {
            chars.next();
            tokens.push(c.to_string());
        } else if c == '%' {
            for c in chars.by_ref() {
                if c == '\n' || c == '\r' {
                    break;
                }
            }
        } else {
            let mut w = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '<' || c == '[' || c == ']' {
                    break;
                }
                w.push(c);
                chars.next();
            }
            tokens.push(w);
        }
    }

    let mut cmap = CMap::default();
    let mut i = 0;
    let hex = |t: &String| t.strip_prefix('<').map(str::to_string);
    while i < tokens.len() {
        match tokens[i].as_str() {
            "begincodespacerange" => {
                i += 1;
                while i + 1 < tokens.len() && tokens[i] != "endcodespacerange" {
                    if let Some(lo) = hex(&tokens[i]) {
                        let len = lo.len() / 2;
                        if len > 0 && !cmap.code_lengths.contains(&len) {
                            cmap.code_lengths.push(len);
                        }
                    }
                    i += 2;
                }
            }
            "beginbfchar" => {
                i += 1;
                while i + 1 < tokens.len() && tokens[i] != "endbfchar" {
                    if let (Some(src), Some(dst)) = (hex(&tokens[i]), hex(&tokens[i + 1])) {
                        cmap.chars.insert(
                            hex_to_u32(&src),
                            String::from_utf16_lossy(&hex_to_utf16(&dst)),
                        );
                    }
                    i += 2;
                }
            }
            "beginbfrange" => {
                i += 1;
                while i + 2 < tokens.len() && tokens[i] != "endbfrange" {
                    let (Some(lo), Some(hi)) = (hex(&tokens[i]), hex(&tokens[i + 1])) else {
                        break;
                    };
                    let (lo, hi) = (hex_to_u32(&lo), hex_to_u32(&hi));
                    if tokens[i + 2] == "[" {
                        i += 3;
                        let mut code = lo;
                        while i < tokens.len() && tokens[i] != "]" {
                            if let Some(d) = hex(&tokens[i]) {
                                cmap.chars
                                    .insert(code, String::from_utf16_lossy(&hex_to_utf16(&d)));
                            }
                            code += 1;
                            i += 1;
                        }
                        i += 1;
                    } else {
                        if let Some(d) = hex(&tokens[i + 2])
                            && hi >= lo
                            && hi - lo < 0x1_0000
                        {
                            cmap.ranges.push((lo, hi, hex_to_utf16(&d)));
                        }
                        i += 3;
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    cmap
}

/// Predefined (non-Identity) CMap families that map byte strings to
/// characters through a legacy encoding.
#[derive(Clone, Copy, PartialEq)]
enum Legacy {
    ShiftJis,
    EucJp,
    Gbk,
    Big5,
    EucKr,
    Ucs2,
}

#[derive(Clone, Copy, PartialEq)]
enum Ordering {
    Japan1,
    Other,
}

enum Kind {
    Simple,
    Cid,
}

pub struct Font {
    pub bold: bool,
    pub italic: bool,
    pub mono: bool,
    kind: Kind,
    first_char: u32,
    widths: Vec<f32>,
    missing_width: f32,
    has_widths: bool,
    table: Vec<Option<String>>,
    to_unicode: Option<CMap>,
    cid_widths: HashMap<u32, f32>,
    default_width: f32,
    ordering: Ordering,
    legacy: Option<Legacy>,
}

impl Font {
    pub fn load(doc: &Document, dict: &Dictionary) -> Font {
        let base_name = dict_get(doc, dict, b"BaseFont")
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .unwrap_or_default();
        let subtype = dict_get(doc, dict, b"Subtype")
            .and_then(|o| o.as_name().ok())
            .unwrap_or(b"");
        let is_cid = subtype == b"Type0";

        let (mut desc_flags, mut weight_hint) = (0i64, 0f32);
        let mut descendant: Option<&Dictionary> = None;
        if is_cid {
            descendant = dict_get(doc, dict, b"DescendantFonts")
                .and_then(|o| o.as_array().ok())
                .and_then(|a| a.first())
                .map(|o| resolve(doc, o))
                .and_then(|o| o.as_dict().ok());
        }
        let desc_src = descendant.unwrap_or(dict);
        if let Some(fd) = dict_get(doc, desc_src, b"FontDescriptor").and_then(|o| o.as_dict().ok())
        {
            desc_flags = dict_get(doc, fd, b"Flags")
                .and_then(|o| o.as_i64().ok())
                .unwrap_or(0);
            weight_hint = dict_get(doc, fd, b"FontWeight")
                .and_then(num)
                .unwrap_or(0.0);
        }

        let lower = base_name.to_lowercase();
        let bold = lower.contains("bold")
            || lower.contains("black")
            || lower.contains("heavy")
            || lower.contains("semibold")
            || lower.contains("demi")
            || weight_hint >= 600.0
            || desc_flags & (1 << 18) != 0;
        let italic =
            lower.contains("italic") || lower.contains("oblique") || desc_flags & (1 << 6) != 0;
        let mono = lower.contains("courier")
            || lower.contains("mono")
            || lower.contains("consolas")
            || lower.contains("menlo")
            || desc_flags & 1 != 0 && !is_cid;

        let to_unicode = dict_get(doc, dict, b"ToUnicode")
            .and_then(|o| o.as_stream().ok())
            .and_then(|s| {
                s.decompressed_content()
                    .ok()
                    .or_else(|| Some(s.content.clone()))
            })
            .map(|d| parse_cmap(&d));

        let mut font = Font {
            bold,
            italic,
            mono,
            kind: if is_cid { Kind::Cid } else { Kind::Simple },
            first_char: 0,
            widths: Vec::new(),
            missing_width: 0.0,
            has_widths: false,
            table: Vec::new(),
            to_unicode,
            cid_widths: HashMap::new(),
            default_width: 1000.0,
            ordering: Ordering::Other,
            legacy: None,
        };

        if is_cid {
            font.load_cid(doc, dict, descendant);
        } else {
            font.load_simple(doc, dict);
        }
        font
    }

    fn load_simple(&mut self, doc: &Document, dict: &Dictionary) {
        self.first_char = dict_get(doc, dict, b"FirstChar")
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(0) as u32;
        if let Some(arr) = dict_get(doc, dict, b"Widths").and_then(|o| o.as_array().ok()) {
            self.widths = arr
                .iter()
                .map(|o| num(resolve(doc, o)).unwrap_or(0.0))
                .collect();
            self.has_widths = !self.widths.is_empty();
        }
        if let Some(fd) = dict_get(doc, dict, b"FontDescriptor").and_then(|o| o.as_dict().ok()) {
            self.missing_width = dict_get(doc, fd, b"MissingWidth")
                .and_then(num)
                .unwrap_or(0.0);
        }

        // Base encoding.
        let mut base = "Standard";
        let mut explicit_base = false;
        let mut differences: Option<&Vec<Object>> = None;
        match dict_get(doc, dict, b"Encoding") {
            Some(Object::Name(n)) => {
                base = encoding_name(n);
                explicit_base = true;
            }
            Some(Object::Dictionary(d)) => {
                if let Some(n) = dict_get(doc, d, b"BaseEncoding").and_then(|o| o.as_name().ok()) {
                    base = encoding_name(n);
                    explicit_base = true;
                }
                differences = dict_get(doc, d, b"Differences").and_then(|o| o.as_array().ok());
            }
            _ => {}
        }
        let mut table: Vec<Option<String>> = (0..=255u8).map(|b| base_char(base, b)).collect();
        // Without an explicit base encoding the embedded font program's own
        // encoding applies (e.g. Ghostscript output with fi/fl at odd codes).
        if !explicit_base && let Some(builtin) = type1_builtin_encoding(doc, dict) {
            for (slot, b) in table.iter_mut().zip(builtin) {
                if b.is_some() {
                    *slot = b;
                }
            }
        }
        if let Some(diff) = differences {
            let mut code = 0usize;
            for o in diff {
                match resolve(doc, o) {
                    Object::Integer(i) => code = (*i).max(0) as usize,
                    Object::Name(n) => {
                        if code < 256 {
                            table[code] =
                                super::glyphnames::name_to_unicode(&String::from_utf8_lossy(n));
                        }
                        code += 1;
                    }
                    _ => {}
                }
            }
        }
        self.table = table;
    }

    fn load_cid(&mut self, doc: &Document, dict: &Dictionary, desc: Option<&Dictionary>) {
        if let Some(enc) = dict_get(doc, dict, b"Encoding").and_then(|o| o.as_name().ok()) {
            let enc = String::from_utf8_lossy(enc);
            self.legacy = if enc.contains("UCS2") || enc.contains("UTF16") {
                Some(Legacy::Ucs2)
            } else if enc.contains("RKSJ") {
                Some(Legacy::ShiftJis)
            } else if enc.starts_with("EUC-") || enc.starts_with("EUC") && enc.contains("-H") {
                Some(Legacy::EucJp)
            } else if enc.starts_with("GBK") || enc.starts_with("GBpc") || enc.starts_with("GB-") {
                Some(Legacy::Gbk)
            } else if enc.starts_with("B5") || enc.contains("ETen") || enc.starts_with("HK") {
                Some(Legacy::Big5)
            } else if enc.starts_with("KSC") {
                Some(Legacy::EucKr)
            } else {
                None
            };
        }
        let Some(desc) = desc else { return };
        if let Some(si) = dict_get(doc, desc, b"CIDSystemInfo").and_then(|o| o.as_dict().ok())
            && let Some(Object::String(s, _)) = dict_get(doc, si, b"Ordering")
            && s == b"Japan1"
        {
            self.ordering = Ordering::Japan1;
        }
        if let Some(dw) = dict_get(doc, desc, b"DW").and_then(num) {
            self.default_width = dw;
        }
        if let Some(w) = dict_get(doc, desc, b"W").and_then(|o| o.as_array().ok()) {
            let items: Vec<&Object> = w.iter().map(|o| resolve(doc, o)).collect();
            let mut i = 0;
            while i < items.len() {
                let Some(first) = num(items[i]) else { break };
                match items.get(i + 1) {
                    Some(Object::Array(list)) => {
                        for (k, v) in list.iter().enumerate() {
                            if let Some(v) = num(resolve(doc, v)) {
                                self.cid_widths.insert(first as u32 + k as u32, v);
                            }
                        }
                        i += 2;
                    }
                    Some(last) => {
                        if let (Some(last), Some(v)) =
                            (num(last), items.get(i + 2).and_then(|o| num(o)))
                        {
                            let (lo, hi) = (first as u32, (last as u32).min(first as u32 + 70_000));
                            for cid in lo..=hi {
                                self.cid_widths.insert(cid, v);
                            }
                        }
                        i += 3;
                    }
                    None => break,
                }
            }
        }
    }

    pub fn decode(&self, bytes: &[u8]) -> Vec<DecodedGlyph> {
        match self.kind {
            Kind::Simple => bytes.iter().map(|&b| self.decode_simple(b)).collect(),
            Kind::Cid => self.decode_cid(bytes),
        }
    }

    fn decode_simple(&self, b: u8) -> DecodedGlyph {
        let mapped = self
            .to_unicode
            .as_ref()
            .and_then(|m| m.lookup(b as u32))
            .or_else(|| self.table.get(b as usize).cloned().flatten());
        let text = mapped.unwrap_or_default();
        let idx = (b as u32).wrapping_sub(self.first_char) as usize;
        let width = if self.has_widths {
            match self.widths.get(idx) {
                Some(w) if *w > 0.0 => *w,
                _ => self.missing_width,
            }
        } else {
            standard_width(&text, self.mono)
        };
        DecodedGlyph {
            text: expand_ligature(text),
            width,
            is_space: b == 32,
        }
    }

    fn decode_cid(&self, bytes: &[u8]) -> Vec<DecodedGlyph> {
        let mut out = Vec::new();
        let mut i = 0;
        let fixed2 = self.legacy.is_none()
            && self
                .to_unicode
                .as_ref()
                .is_none_or(|m| m.code_lengths.is_empty() || m.code_lengths == [2]);
        while i < bytes.len() {
            // Legacy multi-byte encodings.
            if let Some(leg) = self.legacy.filter(|l| *l != Legacy::Ucs2) {
                let single = match leg {
                    Legacy::ShiftJis => bytes[i] < 0x80 || (0xA1..=0xDF).contains(&bytes[i]),
                    _ => bytes[i] < 0x80,
                };
                let len = if single || i + 1 >= bytes.len() { 1 } else { 2 };
                let slice = &bytes[i..i + len];
                let enc = match leg {
                    Legacy::ShiftJis => encoding_rs::SHIFT_JIS,
                    Legacy::EucJp => encoding_rs::EUC_JP,
                    Legacy::Gbk => encoding_rs::GBK,
                    Legacy::Big5 => encoding_rs::BIG5,
                    _ => encoding_rs::EUC_KR,
                };
                // The font's own ToUnicode map is authoritative; the legacy
                // character set is only the fallback for unmapped codes.
                let code = slice.iter().fold(0u32, |acc, &b| (acc << 8) | b as u32);
                let text = match self.to_unicode.as_ref().and_then(|m| m.lookup(code)) {
                    Some(mapped) => mapped,
                    None => enc.decode(slice).0.into_owned(),
                };
                out.push(DecodedGlyph {
                    text,
                    width: if len == 1 { 500.0 } else { self.default_width },
                    is_space: len == 1 && slice[0] == 32,
                });
                i += len;
                continue;
            }

            let len = if fixed2 || self.legacy == Some(Legacy::Ucs2) {
                2.min(bytes.len() - i)
            } else {
                self.next_code_len(&bytes[i..])
            };
            let code = bytes[i..i + len]
                .iter()
                .fold(0u32, |acc, &b| (acc << 8) | b as u32);
            i += len;

            let text = if self.legacy == Some(Legacy::Ucs2) {
                char::from_u32(code).map(String::from).unwrap_or_default()
            } else {
                self.to_unicode
                    .as_ref()
                    .and_then(|m| m.lookup(code))
                    .or_else(|| {
                        (self.ordering == Ordering::Japan1)
                            .then(|| japan1_to_char(code))
                            .flatten()
                            .map(String::from)
                    })
                    .unwrap_or_default()
            };
            let width = self
                .cid_widths
                .get(&code)
                .copied()
                .unwrap_or(self.default_width);
            out.push(DecodedGlyph {
                text: expand_ligature(text),
                width,
                is_space: len == 1 && code == 32,
            });
        }
        out
    }

    fn next_code_len(&self, rest: &[u8]) -> usize {
        let Some(map) = &self.to_unicode else {
            return 2.min(rest.len());
        };
        for len in [1usize, 2, 3, 4] {
            if len <= rest.len() && map.code_lengths.contains(&len) {
                // Prefer the shortest codespace length whose code is mapped.
                let code = rest[..len].iter().fold(0u32, |a, &b| (a << 8) | b as u32);
                if map.lookup(code).is_some() || map.code_lengths.len() == 1 {
                    return len;
                }
            }
        }
        map.code_lengths
            .iter()
            .copied()
            .min()
            .unwrap_or(2)
            .min(rest.len())
    }
}

/// Built-in encoding of the embedded font program: Type 1 clear-text
/// `dup <code> /<name> put` entries or a CFF Encoding table.
fn type1_builtin_encoding(doc: &Document, dict: &Dictionary) -> Option<Vec<Option<String>>> {
    let fd = dict_get(doc, dict, b"FontDescriptor")?.as_dict().ok()?;
    if let Some(cff) = dict_get(doc, fd, b"FontFile3").and_then(|o| o.as_stream().ok()) {
        let data = cff.decompressed_content().ok()?;
        let names = super::cff::builtin_encoding(&data)?;
        return Some(
            names
                .into_iter()
                .map(|n| n.and_then(|n| super::glyphnames::name_to_unicode(&n)))
                .collect(),
        );
    }
    let stream = dict_get(doc, fd, b"FontFile")?.as_stream().ok()?;
    let data = stream.decompressed_content().ok()?;
    let end = data
        .windows(5)
        .position(|w| w == b"eexec")
        .unwrap_or(data.len().min(65_536));
    let header = String::from_utf8_lossy(&data[..end]);
    let start = header.find("/Encoding")?;
    let section = &header[start..];
    if section.starts_with("/Encoding StandardEncoding") {
        return None;
    }
    let mut table: Vec<Option<String>> = vec![None; 256];
    let mut found = false;
    let mut tokens = section.split_whitespace();
    while let Some(t) = tokens.next() {
        if t == "dup" {
            let (Some(code), Some(name)) = (tokens.next(), tokens.next()) else {
                break;
            };
            if let (Ok(code), Some(name)) = (code.parse::<usize>(), name.strip_prefix('/'))
                && code < 256
            {
                table[code] = super::glyphnames::name_to_unicode(name);
                found = true;
            }
        } else if t == "readonly" || t == "def" && found {
            break;
        }
    }
    found.then_some(table)
}

fn encoding_name(n: &[u8]) -> &'static str {
    match n {
        b"WinAnsiEncoding" => "WinAnsi",
        b"MacRomanEncoding" => "MacRoman",
        _ => "Standard",
    }
}

fn base_char(base: &str, b: u8) -> Option<String> {
    if b < 0x20 {
        return None;
    }
    if b < 0x80 {
        return Some(match (base, b) {
            ("Standard", 0x27) => '\u{2019}'.to_string(),
            ("Standard", 0x60) => '\u{2018}'.to_string(),
            _ => (b as char).to_string(),
        });
    }
    let enc = match base {
        "WinAnsi" => encoding_rs::WINDOWS_1252,
        "MacRoman" => encoding_rs::MACINTOSH,
        _ => encoding_rs::WINDOWS_1252,
    };
    let buf = [b];
    let (s, _, bad) = enc.decode(&buf);
    (!bad && s != "\u{FFFD}").then(|| s.into_owned())
}

fn expand_ligature(s: String) -> String {
    if !s.chars().any(|c| ('\u{FB00}'..='\u{FB06}').contains(&c)) {
        return s;
    }
    s.chars()
        .map(|c| match c {
            '\u{FB00}' => "ff".to_string(),
            '\u{FB01}' => "fi".to_string(),
            '\u{FB02}' => "fl".to_string(),
            '\u{FB03}' => "ffi".to_string(),
            '\u{FB04}' => "ffl".to_string(),
            '\u{FB05}' | '\u{FB06}' => "st".to_string(),
            c => c.to_string(),
        })
        .collect()
}

/// Helvetica widths for ASCII 32..=126 (used when a font has no /Widths,
/// e.g. the standard 14 fonts).
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278, 556, 556, 556,
    556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556, 1015, 667, 667, 722, 722, 667,
    611, 778, 722, 278, 500, 667, 556, 833, 722, 778, 667, 778, 722, 667, 611, 722, 667, 944, 667,
    667, 611, 278, 278, 278, 469, 556, 333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500,
    222, 833, 556, 556, 556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

fn standard_width(text: &str, mono: bool) -> f32 {
    if mono {
        return 600.0;
    }
    match text.chars().next() {
        Some(c) if (' '..='~').contains(&c) => HELVETICA[(c as u32 - 32) as usize] as f32,
        Some(c) if c as u32 >= 0x2E80 => 1000.0,
        Some(_) => 556.0,
        None => 500.0,
    }
}

/// Adobe-Japan1 CID → Unicode, generated from Adobe's `Adobe-Japan1-UCS2`
/// CMap (BSD-3-Clause, Copyright 1990-2023 Adobe; see NOTICE in the
/// repository root). Layout: u16 count, `count` × u16 code points (0 = unmapped),
/// u16 n, then `n` × (u16 cid, u32 code point) for characters outside the BMP.
static JAPAN1_UCS2: &[u8] = include_bytes!("japan1_ucs2.bin");

fn japan1_to_char(cid: u32) -> Option<char> {
    let table = JAPAN1_UCS2;
    let be16 = |p: usize| {
        table
            .get(p..p + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]) as u32)
    };
    let count = be16(0)?;
    if cid >= count {
        return None;
    }
    let big_at = 2 + count as usize * 2;
    let n_big = be16(big_at)? as usize;
    for i in 0..n_big {
        let p = big_at + 2 + i * 6;
        if be16(p)? == cid {
            let b = table.get(p + 2..p + 6)?;
            return char::from_u32(u32::from_be_bytes([b[0], b[1], b[2], b[3]]));
        }
    }
    match be16(2 + cid as usize * 2)? {
        0 => None,
        u => char::from_u32(u),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Stream, dictionary};

    fn type0_with_tounicode(cmap: &str) -> (Document, Dictionary) {
        let mut doc = Document::with_version("1.5");
        let id = doc.add_object(Object::Stream(Stream::new(
            dictionary! {},
            cmap.as_bytes().to_vec(),
        )));
        let dict = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => "Test-Bold",
            "Encoding" => "Identity-H",
            "ToUnicode" => Object::Reference(id),
        };
        (doc, dict)
    }

    #[test]
    fn to_unicode_bfchar_bfrange_and_ligature() {
        let cmap = "1 begincodespacerange <0000> <FFFF> endcodespacerange\n\
                    2 beginbfchar <0001> <0041> <0002> <FB01> endbfchar\n\
                    1 beginbfrange <0010> <0012> <0061> endbfrange\n\
                    1 beginbfrange <0020> <0021> [<0078> <0079>] endbfrange";
        let (doc, dict) = type0_with_tounicode(cmap);
        let font = Font::load(&doc, &dict);
        let text = |bytes: &[u8]| {
            font.decode(bytes)
                .into_iter()
                .map(|g| g.text)
                .collect::<String>()
        };
        assert_eq!(text(&[0, 1]), "A");
        assert_eq!(text(&[0, 2]), "fi"); // ligature expanded
        assert_eq!(text(&[0, 0x10, 0, 0x12]), "ac");
        assert_eq!(text(&[0, 0x20, 0, 0x21]), "xy");
        assert!(font.bold);
    }

    #[test]
    fn cmap_sections_are_found_regardless_of_token_parity() {
        // An odd and an even number of tokens before `begin…` must both work.
        for prefix in ["", "/CIDInit /ProcSet findresource begin", "x y z"] {
            let cmap = format!("{prefix} 1 beginbfchar <0003> <0058> endbfchar");
            let c = parse_cmap(cmap.as_bytes());
            assert_eq!(c.lookup(3).as_deref(), Some("X"), "prefix {prefix:?}");
        }
    }

    #[test]
    fn cid_widths_come_from_w_array() {
        let mut doc = Document::with_version("1.5");
        let desc = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "CIDFontType2",
            "DW" => 1000,
            "W" => vec![Object::Integer(5), Object::Array(vec![Object::Integer(250), Object::Integer(300)]),
                        Object::Integer(9), Object::Integer(11), Object::Integer(400)],
        });
        let dict = dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "Encoding" => "Identity-H",
            "DescendantFonts" => vec![Object::Reference(desc)],
        };
        let font = Font::load(&doc, &dict);
        let w = |code: u8| font.decode(&[0, code])[0].width;
        assert_eq!((w(5), w(6), w(10), w(99)), (250.0, 300.0, 400.0, 1000.0));
    }

    #[test]
    fn legacy_cjk_font_prefers_to_unicode_over_the_charset() {
        // 0x8140 is an ideographic space in Shift-JIS, but this font's own
        // ToUnicode map says ★; 0x82A0 is unmapped and falls back to Shift-JIS.
        let cmap = "1 begincodespacerange <00> <FFFF> endcodespacerange\n\
                    1 beginbfchar <8140> <2605> endbfchar";
        let mut doc = Document::with_version("1.5");
        let id = doc.add_object(Object::Stream(Stream::new(
            dictionary! {},
            cmap.as_bytes().to_vec(),
        )));
        let dict = dictionary! {
            "Type" => "Font",
            "Subtype" => "Type0",
            "BaseFont" => "Test",
            "Encoding" => "90ms-RKSJ-H",
            "ToUnicode" => Object::Reference(id),
        };
        let font = Font::load(&doc, &dict);
        let text: String = font
            .decode(&[0x81, 0x40, b'A', 0x82, 0xA0])
            .into_iter()
            .map(|g| g.text)
            .collect();
        assert_eq!(text, "★Aあ");
    }

    #[test]
    fn japan1_cids_map_to_unicode() {
        assert_eq!(japan1_to_char(1), Some(' '));
        assert_eq!(japan1_to_char(34), Some('A'));
        assert_eq!(japan1_to_char(3284), Some('日'));
        assert_eq!(japan1_to_char(842), Some('ぁ'));
        assert_eq!(japan1_to_char(u32::MAX), None);
    }

    #[test]
    fn differences_map_glyph_names() {
        let doc = Document::with_version("1.5");
        let dict = dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Times-Roman",
            "Encoding" => dictionary! {
                "Type" => "Encoding",
                "Differences" => vec![Object::Integer(140), "fi".into(), "bullet".into(), "uni3042".into()],
            },
        };
        let font = Font::load(&doc, &dict);
        let text: String = font
            .decode(&[140, 141, 142, b'z'])
            .into_iter()
            .map(|g| g.text)
            .collect();
        assert_eq!(text, "fi•あz");
    }

    #[test]
    fn simple_font_without_widths_uses_helvetica_metrics() {
        let doc = Document::with_version("1.5");
        let dict =
            dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" };
        let font = Font::load(&doc, &dict);
        let g = font.decode(b"i M");
        assert_eq!((g[0].width, g[2].width), (222.0, 833.0));
        assert!(g[1].is_space);
    }
}
