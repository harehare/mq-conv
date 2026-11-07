//! Helpers shared by the converters that read XML with `quick-xml`.

use quick_xml::events::BytesRef;

/// The text an entity or character reference stands for (`&amp;`, `&#x41;`).
///
/// `quick-xml` reports these as their own events instead of folding them into
/// the surrounding text, so a reader that only looks at text events loses
/// them. `None` for an entity the document would have to define itself.
pub fn entity_text(reference: &BytesRef) -> Option<String> {
    if let Ok(Some(c)) = reference.resolve_char_ref() {
        return Some(c.to_string());
    }
    let text = match &**reference {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => return None,
    };
    Some(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn predefined_entities_and_character_references() {
        let text = |s: &str| entity_text(&BytesRef::new(s));
        assert_eq!(text("amp").as_deref(), Some("&"));
        assert_eq!(text("lt").as_deref(), Some("<"));
        assert_eq!(text("#x41").as_deref(), Some("A"));
        assert_eq!(text("#12354").as_deref(), Some("あ"));
        assert_eq!(text("nbsp"), None);
    }
}
