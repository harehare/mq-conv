use std::io::Write;

use crate::converter::Converter;
use crate::error::{Error, Result};
use crate::formats::structured;

pub struct JsonConverter;

impl Converter for JsonConverter {
    fn format_name(&self) -> &'static str {
        "json"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let value: serde_json::Value =
            serde_json::from_slice(input).map_err(|e| Error::Conversion {
                format: "json",
                message: e.to_string(),
            })?;

        let structured_value = structured::Value::from(value);
        structured::write_value_as_markdown(writer, &structured_value)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converter::Converter;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    fn convert(input: &str) -> String {
        let converter = JsonConverter;
        let mut output = Vec::new();
        converter.convert(input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[rstest]
    #[case::primitive_string(r#""hello""#, "hello\n")]
    #[case::primitive_integer("42", "42\n")]
    #[case::primitive_bool("true", "true\n")]
    #[case::null("null", "\n")]
    fn test_primitive(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    #[case::flat_object(
        r#"{"name":"Alice","age":30}"#,
        "| Key | Value |\n|---|---|\n| name | Alice |\n| age | 30 |\n\n"
    )]
    #[case::nested_object(
        r#"{"name":"Alice","address":{"city":"Tokyo","zip":"100"}}"#,
        "| Key | Value |\n|---|---|\n| name | Alice |\n\n# address\n\n| Key | Value |\n|---|---|\n| city | Tokyo |\n| zip | 100 |\n\n"
    )]
    #[case::object_with_array_of_primitives(
        r#"{"tags":["rust","cli"]}"#,
        "# tags\n\n- rust\n- cli\n\n"
    )]
    #[case::object_with_array_of_objects(
        r#"{"users":[{"name":"Alice","role":"admin"},{"name":"Bob","role":"user"}]}"#,
        "# users\n\n| name | role |\n|---|---|\n| Alice | admin |\n| Bob | user |\n\n"
    )]
    fn test_object(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    #[case::array_of_primitives(r#"["a","b","c"]"#, "- a\n- b\n- c\n\n")]
    #[case::array_of_objects(
        r#"[{"id":1,"name":"x"},{"id":2,"name":"y"}]"#,
        "| id | name |\n|---|---|\n| 1 | x |\n| 2 | y |\n\n"
    )]
    #[case::empty_array("[]", "*empty*\n")]
    fn test_array(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    #[case::pipe_in_value(
        r#"{"cmd":"a|b"}"#,
        "| Key | Value |\n|---|---|\n| cmd | a\\|b |\n\n"
    )]
    fn test_escape(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    fn test_deep_nesting() {
        let input = r#"{"a":{"b":{"c":{"d":{"e":{"f":{"g":"deep"}}}}}}}"#;
        let output = convert(input);
        assert!(output.contains("###### f"));
        assert!(output.contains("deep"));
    }

    #[rstest]
    fn test_mixed_array() {
        let output = convert(r#"[1,{"key":"val"}]"#);
        assert!(output.contains("- 1"));
        assert!(output.contains("| Key | Value |"));
        assert!(output.contains("| key | val |"));
    }
}
