use std::io::Write;

use crate::converter::Converter;
use crate::error::{Error, Result};
use crate::formats::structured;

pub struct YamlConverter;

impl Converter for YamlConverter {
    fn format_name(&self) -> &'static str {
        "yaml"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let value: serde_yaml::Value =
            serde_yaml::from_slice(input).map_err(|e| Error::Conversion {
                format: "yaml",
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
        let converter = YamlConverter;
        let mut output = Vec::new();
        converter.convert(input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[rstest]
    #[case::flat_mapping(
        "name: Alice\nage: 30",
        "| Key | Value |\n|---|---|\n| name | Alice |\n| age | 30 |\n\n"
    )]
    #[case::nested_mapping(
        "user:\n  name: Bob\n  city: Tokyo",
        "# user\n\n| Key | Value |\n|---|---|\n| name | Bob |\n| city | Tokyo |\n\n"
    )]
    #[case::sequence_of_scalars(
        "items:\n  - apple\n  - banana",
        "# items\n\n- apple\n- banana\n\n"
    )]
    #[case::sequence_of_mappings(
        "users:\n  - name: A\n    id: 1\n  - name: B\n    id: 2",
        "# users\n\n| name | id |\n|---|---|\n| A | 1 |\n| B | 2 |\n\n"
    )]
    fn test_conversion(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    #[case::top_level_sequence("- a\n- b\n- c", "- a\n- b\n- c\n\n")]
    #[case::scalar_string("hello", "hello\n")]
    #[case::scalar_integer("42", "42\n")]
    #[case::null_value(
        "key: null",
        "| Key | Value |\n|---|---|\n| key |  |\n\n"
    )]
    fn test_edge_cases(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    fn test_non_string_keys() {
        let output = convert("true: yes\nfalse: no");
        assert!(output.contains("true"));
        assert!(output.contains("false"));
    }
}
