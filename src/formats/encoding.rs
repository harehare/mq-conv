use encoding_rs::Encoding;

pub fn decode_text(bytes: &[u8]) -> String {
    decode_with(bytes, None)
}

pub fn decode_html(bytes: &[u8]) -> String {
    decode_with(bytes, sniff_meta_charset(bytes))
}

fn decode_with(bytes: &[u8], hint: Option<&'static Encoding>) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }

    let encoding = Encoding::for_bom(bytes)
        .map(|(enc, _)| enc)
        .or(hint)
        .unwrap_or_else(|| detect_encoding(bytes));

    let (decoded, _, _) = encoding.decode(bytes);
    decoded.into_owned()
}

fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    detector.guess(None, true)
}

fn sniff_meta_charset(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = &bytes[..bytes.len().min(1024)];
    let lower: Vec<u8> = head.iter().map(u8::to_ascii_lowercase).collect();
    let start = find_subslice(&lower, b"charset=")? + b"charset=".len();
    let rest = &head[start..];
    let rest = rest
        .strip_prefix(b"\"")
        .or_else(|| rest.strip_prefix(b"'"))
        .unwrap_or(rest);
    let end = rest
        .iter()
        .position(|&b| matches!(b, b'"' | b'\'' | b'>' | b';' | b' '))
        .unwrap_or(rest.len());
    Encoding::for_label(&rest[..end])
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_unchanged() {
        assert_eq!(decode_text("日本語".as_bytes()), "日本語");
    }

    #[test]
    fn decodes_shift_jis() {
        let (bytes, _, had_errors) = encoding_rs::SHIFT_JIS.encode("日本語のテスト");
        assert!(!had_errors);
        assert_eq!(decode_text(&bytes), "日本語のテスト");
    }

    #[test]
    fn decodes_euc_jp_via_meta_charset() {
        let (body, _, had_errors) = encoding_rs::EUC_JP.encode("日本語のページ");
        assert!(!had_errors);
        let mut html = b"<html><head><meta charset=\"euc-jp\"></head><body>".to_vec();
        html.extend_from_slice(&body);
        html.extend_from_slice(b"</body></html>");
        assert!(decode_html(&html).contains("日本語のページ"));
    }
}
