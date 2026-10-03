//! Minimal CFF reader: recovers the built-in code → glyph-name encoding of a
//! Type1C font program (used when a PDF font has no /Encoding entry).

const STD_STRINGS: [&str; 229] = [
    ".notdef",
    "space",
    "exclam",
    "quotedbl",
    "numbersign",
    "dollar",
    "percent",
    "ampersand",
    "quoteright",
    "parenleft",
    "parenright",
    "asterisk",
    "plus",
    "comma",
    "hyphen",
    "period",
    "slash",
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "colon",
    "semicolon",
    "less",
    "equal",
    "greater",
    "question",
    "at",
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "bracketleft",
    "backslash",
    "bracketright",
    "asciicircum",
    "underscore",
    "quoteleft",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "braceleft",
    "bar",
    "braceright",
    "asciitilde",
    "exclamdown",
    "cent",
    "sterling",
    "fraction",
    "yen",
    "florin",
    "section",
    "currency",
    "quotesingle",
    "quotedblleft",
    "guillemotleft",
    "guilsinglleft",
    "guilsinglright",
    "fi",
    "fl",
    "endash",
    "dagger",
    "daggerdbl",
    "periodcentered",
    "paragraph",
    "bullet",
    "quotesinglbase",
    "quotedblbase",
    "quotedblright",
    "guillemotright",
    "ellipsis",
    "perthousand",
    "questiondown",
    "grave",
    "acute",
    "circumflex",
    "tilde",
    "macron",
    "breve",
    "dotaccent",
    "dieresis",
    "ring",
    "cedilla",
    "hungarumlaut",
    "ogonek",
    "caron",
    "emdash",
    "AE",
    "ordfeminine",
    "Lslash",
    "Oslash",
    "OE",
    "ordmasculine",
    "ae",
    "dotlessi",
    "lslash",
    "oslash",
    "oe",
    "germandbls",
    "onesuperior",
    "logicalnot",
    "mu",
    "trademark",
    "Eth",
    "onehalf",
    "plusminus",
    "Thorn",
    "onequarter",
    "divide",
    "brokenbar",
    "degree",
    "thorn",
    "threequarters",
    "twosuperior",
    "registered",
    "minus",
    "eth",
    "multiply",
    "threesuperior",
    "copyright",
    "Aacute",
    "Acircumflex",
    "Adieresis",
    "Agrave",
    "Aring",
    "Atilde",
    "Ccedilla",
    "Eacute",
    "Ecircumflex",
    "Edieresis",
    "Egrave",
    "Iacute",
    "Icircumflex",
    "Idieresis",
    "Igrave",
    "Ntilde",
    "Oacute",
    "Ocircumflex",
    "Odieresis",
    "Ograve",
    "Otilde",
    "Scaron",
    "Uacute",
    "Ucircumflex",
    "Udieresis",
    "Ugrave",
    "Yacute",
    "Ydieresis",
    "Zcaron",
    "aacute",
    "acircumflex",
    "adieresis",
    "agrave",
    "aring",
    "atilde",
    "ccedilla",
    "eacute",
    "ecircumflex",
    "edieresis",
    "egrave",
    "iacute",
    "icircumflex",
    "idieresis",
    "igrave",
    "ntilde",
    "oacute",
    "ocircumflex",
    "odieresis",
    "ograve",
    "otilde",
    "scaron",
    "uacute",
    "ucircumflex",
    "udieresis",
    "ugrave",
    "yacute",
    "ydieresis",
    "zcaron",
];

struct Reader<'a> {
    d: &'a [u8],
}

impl Reader<'_> {
    fn u8(&self, p: usize) -> Option<usize> {
        self.d.get(p).map(|&b| b as usize)
    }
    fn uint(&self, p: usize, n: usize) -> Option<usize> {
        (0..n).try_fold(0usize, |acc, i| Some((acc << 8) | self.u8(p + i)?))
    }

    /// Returns the (start, end) byte range of every INDEX entry and the
    /// position just past the INDEX.
    fn index(&self, pos: usize) -> Option<(Vec<(usize, usize)>, usize)> {
        let count = self.uint(pos, 2)?;
        if count == 0 {
            return Some((Vec::new(), pos + 2));
        }
        let off_size = self.u8(pos + 2)?;
        if !(1..=4).contains(&off_size) {
            return None;
        }
        let base = pos + 3 + (count + 1) * off_size - 1;
        let mut ranges = Vec::with_capacity(count);
        let mut prev = self.uint(pos + 3, off_size)?;
        for i in 1..=count {
            let next = self.uint(pos + 3 + i * off_size, off_size)?;
            ranges.push((base + prev, base + next));
            prev = next;
        }
        Some((ranges, base + prev))
    }
}

/// DICT operands/operators: returns a list of (operator, operands).
fn parse_dict(d: &[u8]) -> Vec<(u16, Vec<f64>)> {
    let mut out = Vec::new();
    let mut operands: Vec<f64> = Vec::new();
    let mut i = 0;
    while i < d.len() {
        let b = d[i];
        match b {
            0..=21 => {
                let op = if b == 12 {
                    i += 1;
                    1200 + *d.get(i).unwrap_or(&0) as u16
                } else {
                    b as u16
                };
                out.push((op, std::mem::take(&mut operands)));
                i += 1;
            }
            28 => {
                if i + 2 < d.len() {
                    operands.push(i16::from_be_bytes([d[i + 1], d[i + 2]]) as f64);
                }
                i += 3;
            }
            29 => {
                if i + 4 < d.len() {
                    operands
                        .push(i32::from_be_bytes([d[i + 1], d[i + 2], d[i + 3], d[i + 4]]) as f64);
                }
                i += 5;
            }
            30 => {
                i += 1;
                while i < d.len() {
                    let (hi, lo) = (d[i] >> 4, d[i] & 0xF);
                    i += 1;
                    if hi == 0xF || lo == 0xF {
                        break;
                    }
                }
                operands.push(0.0);
            }
            32..=246 => {
                operands.push(b as f64 - 139.0);
                i += 1;
            }
            247..=250 => {
                operands.push(
                    ((b as f64 - 247.0) * 256.0) + *d.get(i + 1).unwrap_or(&0) as f64 + 108.0,
                );
                i += 2;
            }
            251..=254 => {
                operands.push(
                    -((b as f64 - 251.0) * 256.0) - *d.get(i + 1).unwrap_or(&0) as f64 - 108.0,
                );
                i += 2;
            }
            _ => i += 1,
        }
    }
    out
}

/// Built-in encoding as 256 glyph names (`None` where the code is unused).
pub fn builtin_encoding(data: &[u8]) -> Option<Vec<Option<String>>> {
    let r = Reader { d: data };
    let hdr_size = r.u8(2)?;
    let (_names, p) = r.index(hdr_size)?;
    let (tops, p) = r.index(p)?;
    let (strings, _p) = r.index(p)?;
    let top = parse_dict(data.get(tops.first()?.0..tops.first()?.1)?);

    let get = |op: u16| {
        top.iter()
            .find(|(o, _)| *o == op)
            .and_then(|(_, v)| v.first().copied())
    };
    if get(1230).is_some() {
        return None; // CID-keyed: no code-based encoding
    }
    let encoding_off = get(16)? as usize;
    if encoding_off <= 1 {
        return None; // Standard / Expert: handled by the caller's base table
    }
    let charset_off = get(15).unwrap_or(0.0) as usize;
    let charstrings_off = get(17)? as usize;
    let (glyphs, _) = r.index(charstrings_off)?;
    let n_glyphs = glyphs.len();

    let sid_name = |sid: usize| -> Option<String> {
        if sid < STD_STRINGS.len() {
            Some(STD_STRINGS[sid].to_string())
        } else if sid >= 391 {
            let (a, b) = *strings.get(sid - 391)?;
            Some(String::from_utf8_lossy(data.get(a..b)?).into_owned())
        } else {
            None
        }
    };

    // gid → SID
    let mut gid_sid: Vec<usize> = vec![0; n_glyphs];
    if charset_off > 2 {
        let fmt = r.u8(charset_off)?;
        let mut p = charset_off + 1;
        let mut gid = 1;
        match fmt {
            0 => {
                while gid < n_glyphs {
                    gid_sid[gid] = r.uint(p, 2)?;
                    p += 2;
                    gid += 1;
                }
            }
            1 | 2 => {
                while gid < n_glyphs {
                    let first = r.uint(p, 2)?;
                    let (left, w) = if fmt == 1 {
                        (r.u8(p + 2)?, 3)
                    } else {
                        (r.uint(p + 2, 2)?, 4)
                    };
                    p += w;
                    for k in 0..=left {
                        if gid >= n_glyphs {
                            break;
                        }
                        gid_sid[gid] = first + k;
                        gid += 1;
                    }
                }
            }
            _ => return None,
        }
    } else {
        for (gid, slot) in gid_sid.iter_mut().enumerate() {
            *slot = gid; // ISOAdobe
        }
    }

    let mut table: Vec<Option<String>> = vec![None; 256];
    let fmt = r.u8(encoding_off)?;
    let mut p = encoding_off + 1;
    match fmt & 0x7F {
        0 => {
            let n = r.u8(p)?;
            p += 1;
            for i in 0..n {
                let code = r.u8(p + i)?;
                if let Some(&sid) = gid_sid.get(i + 1) {
                    table[code] = sid_name(sid);
                }
            }
            p += n;
        }
        1 => {
            let n = r.u8(p)?;
            p += 1;
            let mut gid = 1;
            for i in 0..n {
                let first = r.u8(p + i * 2)?;
                let left = r.u8(p + i * 2 + 1)?;
                for k in 0..=left {
                    if let (Some(&sid), true) = (gid_sid.get(gid), first + k < 256) {
                        table[first + k] = sid_name(sid);
                    }
                    gid += 1;
                }
            }
            p += n * 2;
        }
        _ => return None,
    }
    if fmt & 0x80 != 0 {
        let n = r.u8(p)?;
        p += 1;
        for i in 0..n {
            let code = r.u8(p + i * 3)?;
            let sid = r.uint(p + i * 3 + 1, 2)?;
            table[code] = sid_name(sid);
        }
    }
    Some(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(items: &[&[u8]]) -> Vec<u8> {
        if items.is_empty() {
            return vec![0, 0];
        }
        let mut out = (items.len() as u16).to_be_bytes().to_vec();
        out.push(1);
        let mut off = 1u8;
        out.push(off);
        for it in items {
            off += it.len() as u8;
            out.push(off);
        }
        for it in items {
            out.extend(*it);
        }
        out
    }

    fn int5(v: usize) -> Vec<u8> {
        let mut o = vec![29];
        o.extend((v as i32).to_be_bytes());
        o
    }

    /// Glyphs: .notdef, fi (SID 109), bullet (SID 116) at codes 140 and 131.
    fn sample_cff() -> Vec<u8> {
        let header = vec![1, 0, 4, 1];
        let name = index(&[b"T"]);
        let strings = index(&[]);
        let gsubrs = index(&[]);
        // Top DICT is 3 x (5-byte int + 1-byte operator) = 18 bytes.
        let top_index_len = index(&[&[0u8; 18]]).len();
        let charset_off = header.len() + name.len() + top_index_len + strings.len() + gsubrs.len();
        let charset = vec![0, 0, 109, 0, 116];
        let enc_off = charset_off + charset.len();
        let encoding = vec![0, 2, 140, 131];
        let cs_off = enc_off + encoding.len();
        let charstrings = index(&[&[0x0e], &[0x0e], &[0x0e]]);

        let mut top = Vec::new();
        top.extend(int5(charset_off));
        top.push(15);
        top.extend(int5(enc_off));
        top.push(16);
        top.extend(int5(cs_off));
        top.push(17);

        let mut out = header;
        out.extend(name);
        out.extend(index(&[&top]));
        out.extend(strings);
        out.extend(gsubrs);
        out.extend(charset);
        out.extend(encoding);
        out.extend(charstrings);
        out
    }

    #[test]
    fn reads_builtin_encoding_names() {
        let table = builtin_encoding(&sample_cff()).expect("encoding");
        assert_eq!(table[140].as_deref(), Some("fi"));
        assert_eq!(table[131].as_deref(), Some("bullet"));
        assert_eq!(table[65], None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(builtin_encoding(b"not a cff").is_none());
        assert!(builtin_encoding(&[]).is_none());
    }
}
