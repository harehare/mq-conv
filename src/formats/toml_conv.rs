use std::io::Write;

use crate::converter::Converter;
use crate::error::{Error, Result};
use crate::formats::structured;

pub struct TomlConverter;

impl Converter for TomlConverter {
    fn format_name(&self) -> &'static str {
        "toml"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        let text = std::str::from_utf8(input).map_err(|e| Error::Conversion {
            format: "toml",
            message: e.to_string(),
        })?;

        let value: toml::Value = toml::from_str(text).map_err(|e| Error::Conversion {
            format: "toml",
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
        let converter = TomlConverter;
        let mut output = Vec::new();
        converter.convert(input.as_bytes(), &mut output).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[rstest]
    #[case::flat_keys(
        "name = \"test\"\nversion = \"0.1.0\"",
        "| Key | Value |\n|---|---|\n| name | test |\n| version | 0.1.0 |\n\n"
    )]
    #[case::section(
        "[package]\nname = \"app\"\nversion = \"1.0\"",
        "# package\n\n| Key | Value |\n|---|---|\n| name | app |\n| version | 1.0 |\n\n"
    )]
    #[case::nested_sections(
        "[a]\n[a.b]\nkey = \"val\"",
        "# a\n\n## b\n\n| Key | Value |\n|---|---|\n| key | val |\n\n"
    )]
    #[case::array_of_strings(
        "tags = [\"rust\", \"cli\"]",
        "# tags\n\n- rust\n- cli\n\n"
    )]
    #[case::array_of_tables(
        "[[items]]\nid = 1\nname = \"x\"\n\n[[items]]\nid = 2\nname = \"y\"",
        "# items\n\n| id | name |\n|---|---|\n| 1 | x |\n| 2 | y |\n\n"
    )]
    fn test_conversion(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(convert(input), expected);
    }

    #[rstest]
    #[case::integer("val = 42")]
    #[case::float("val = 3.14")]
    #[case::boolean("val = true")]
    fn test_primitive_types(#[case] input: &str) {
        let output = convert(input);
        assert!(output.contains("| Key | Value |"));
        assert!(output.contains("| val |"));
    }

    #[rstest]
    fn test_inline_table() {
        let output = convert("dep = { version = \"1\", features = [\"full\"] }");
        assert!(output.contains("dep"));
        assert!(output.contains("version"));
    }
}
