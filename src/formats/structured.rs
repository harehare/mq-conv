use std::io::Write;

use crate::error::Result;

/// A format-agnostic value representation for structured data.
/// Each format converter converts its native value type into this enum,
/// then uses `write_value_as_markdown` to produce structured markdown output.
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<Value>),
    /// Key-value pairs preserving insertion order.
    Object(Vec<(String, Value)>),
}

impl Value {
    fn is_primitive(&self) -> bool {
        matches!(
            self,
            Value::Null | Value::Bool(_) | Value::Integer(_) | Value::Float(_) | Value::String(_)
        )
    }

    fn display_primitive(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Integer(n) => n.to_string(),
            Value::Float(f) => f.to_string(),
            Value::String(s) => s.clone(),
            Value::Array(_) | Value::Object(_) => String::new(),
        }
    }
}

/// Write a structured value as markdown to the given writer.
pub fn write_value_as_markdown(writer: &mut dyn Write, value: &Value) -> Result<()> {
    write_value(writer, value, 1)?;
    Ok(())
}

fn write_value(writer: &mut dyn Write, value: &Value, depth: usize) -> Result<()> {
    match value {
        Value::Null => {
            writeln!(writer)?;
        }
        Value::Bool(_) | Value::Integer(_) | Value::Float(_) | Value::String(_) => {
            writeln!(writer, "{}", value.display_primitive())?;
        }
        Value::Array(items) => {
            write_array(writer, items, depth)?;
        }
        Value::Object(entries) => {
            write_object(writer, entries, depth)?;
        }
    }
    Ok(())
}

fn write_object(writer: &mut dyn Write, entries: &[(String, Value)], depth: usize) -> Result<()> {
    // Separate entries into primitive key-value pairs and complex (nested) entries.
    // Group consecutive primitives into a table.
    let mut i = 0;
    while i < entries.len() {
        if entries[i].1.is_primitive() {
            // Collect consecutive primitive entries
            let start = i;
            while i < entries.len() && entries[i].1.is_primitive() {
                i += 1;
            }
            let primitives = &entries[start..i];
            write_kv_table(writer, primitives)?;
            writeln!(writer)?;
        } else {
            let (key, val) = &entries[i];
            write_heading(writer, key, depth)?;
            write_value(writer, val, depth + 1)?;
            i += 1;
        }
    }
    Ok(())
}

fn write_array(writer: &mut dyn Write, items: &[Value], depth: usize) -> Result<()> {
    if items.is_empty() {
        writeln!(writer, "*empty*")?;
        return Ok(());
    }

    // Check if all items are objects with similar keys → render as table
    if let Some(table) = try_as_table(items) {
        write_markdown_table(writer, &table.headers, &table.rows)?;
        writeln!(writer)?;
        return Ok(());
    }

    // Check if all items are primitives → render as bullet list
    if items.iter().all(|v| v.is_primitive()) {
        for item in items {
            writeln!(writer, "- {}", item.display_primitive())?;
        }
        writeln!(writer)?;
        return Ok(());
    }

    // Mixed array: render each item
    for (idx, item) in items.iter().enumerate() {
        match item {
            v if v.is_primitive() => {
                writeln!(writer, "- {}", v.display_primitive())?;
            }
            Value::Object(entries) => {
                write_heading(writer, &format!("{}", idx + 1), depth)?;
                write_object(writer, entries, depth + 1)?;
            }
            Value::Array(inner) => {
                write_heading(writer, &format!("{}", idx + 1), depth)?;
                write_array(writer, inner, depth + 1)?;
            }
            _ => {}
        }
    }

    Ok(())
}

fn write_heading(writer: &mut dyn Write, text: &str, depth: usize) -> Result<()> {
    let level = depth.min(6);
    let hashes = "#".repeat(level);
    writeln!(writer, "{hashes} {text}")?;
    writeln!(writer)?;
    Ok(())
}

/// Write a set of primitive key-value pairs as a markdown table.
fn write_kv_table(writer: &mut dyn Write, entries: &[(String, Value)]) -> Result<()> {
    writeln!(writer, "| Key | Value |")?;
    writeln!(writer, "|---|---|")?;
    for (key, val) in entries {
        let escaped_key = escape_pipe(key);
        let escaped_val = escape_pipe(&val.display_primitive());
        writeln!(writer, "| {escaped_key} | {escaped_val} |")?;
    }
    Ok(())
}

struct TableData {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

/// Try to interpret an array of values as a table (array of objects with common keys).
fn try_as_table(items: &[Value]) -> Option<TableData> {
    // All items must be objects
    let objects: Vec<&Vec<(String, Value)>> = items
        .iter()
        .filter_map(|v| match v {
            Value::Object(entries) => Some(entries),
            _ => None,
        })
        .collect();

    if objects.len() != items.len() || objects.is_empty() {
        return None;
    }

    // All values in the objects must be primitives
    if !objects
        .iter()
        .all(|entries| entries.iter().all(|(_, v)| v.is_primitive()))
    {
        return None;
    }

    // Collect all unique keys preserving order from first object
    let mut headers: Vec<String> = Vec::new();
    for entries in &objects {
        for (key, _) in *entries {
            if !headers.contains(key) {
                headers.push(key.clone());
            }
        }
    }

    let rows: Vec<Vec<String>> = objects
        .iter()
        .map(|entries| {
            headers
                .iter()
                .map(|h| {
                    entries
                        .iter()
                        .find(|(k, _)| k == h)
                        .map(|(_, v)| v.display_primitive())
                        .unwrap_or_default()
                })
                .collect()
        })
        .collect();

    Some(TableData { headers, rows })
}

fn write_markdown_table(
    writer: &mut dyn Write,
    headers: &[String],
    rows: &[Vec<String>],
) -> Result<()> {
    // Header row
    write!(writer, "|")?;
    for h in headers {
        write!(writer, " {} |", escape_pipe(h))?;
    }
    writeln!(writer)?;

    // Separator row
    write!(writer, "|")?;
    for _ in headers {
        write!(writer, "---|")?;
    }
    writeln!(writer)?;

    // Data rows
    for row in rows {
        write!(writer, "|")?;
        for (i, cell) in row.iter().enumerate() {
            if i < headers.len() {
                write!(writer, " {} |", escape_pipe(cell))?;
            }
        }
        writeln!(writer)?;
    }

    Ok(())
}

fn escape_pipe(s: &str) -> String {
    s.replace('|', "\\|")
}

// --- Conversions from format-specific value types ---

#[cfg(feature = "json")]
impl From<serde_json::Value> for Value {
    fn from(v: serde_json::Value) -> Self {
        match v {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else {
                    Value::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_json::Value::String(s) => Value::String(s),
            serde_json::Value::Array(arr) => {
                Value::Array(arr.into_iter().map(Value::from).collect())
            }
            serde_json::Value::Object(map) => {
                Value::Object(map.into_iter().map(|(k, v)| (k, Value::from(v))).collect())
            }
        }
    }
}

#[cfg(feature = "toml_conv")]
impl From<toml::Value> for Value {
    fn from(v: toml::Value) -> Self {
        match v {
            toml::Value::String(s) => Value::String(s),
            toml::Value::Integer(i) => Value::Integer(i),
            toml::Value::Float(f) => Value::Float(f),
            toml::Value::Boolean(b) => Value::Bool(b),
            toml::Value::Datetime(dt) => Value::String(dt.to_string()),
            toml::Value::Array(arr) => Value::Array(arr.into_iter().map(Value::from).collect()),
            toml::Value::Table(map) => {
                Value::Object(map.into_iter().map(|(k, v)| (k, Value::from(v))).collect())
            }
        }
    }
}

#[cfg(feature = "yaml")]
impl From<serde_yaml::Value> for Value {
    fn from(v: serde_yaml::Value) -> Self {
        match v {
            serde_yaml::Value::Null => Value::Null,
            serde_yaml::Value::Bool(b) => Value::Bool(b),
            serde_yaml::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else {
                    Value::Float(n.as_f64().unwrap_or(0.0))
                }
            }
            serde_yaml::Value::String(s) => Value::String(s),
            serde_yaml::Value::Sequence(arr) => {
                Value::Array(arr.into_iter().map(Value::from).collect())
            }
            serde_yaml::Value::Mapping(map) => Value::Object(
                map.into_iter()
                    .map(|(k, v)| {
                        let key = match k {
                            serde_yaml::Value::String(s) => s,
                            serde_yaml::Value::Number(n) => n.to_string(),
                            serde_yaml::Value::Bool(b) => b.to_string(),
                            _ => format!("{k:?}"),
                        };
                        (key, Value::from(v))
                    })
                    .collect(),
            ),
            serde_yaml::Value::Tagged(tagged) => Value::from(tagged.value),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64;

    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    fn render(value: Value) -> String {
        let mut output = Vec::new();
        write_value_as_markdown(&mut output, &value).unwrap();
        String::from_utf8(output).unwrap()
    }

    #[rstest]
    #[case::null_value(Value::Null, "\n")]
    #[case::bool_true(Value::Bool(true), "true\n")]
    #[case::bool_false(Value::Bool(false), "false\n")]
    #[case::integer(Value::Integer(42), "42\n")]
    #[case::float(Value::Float(f64::consts::PI), "3.141592653589793\n")]
    #[case::string(Value::String("hello".into()), "hello\n")]
    fn test_primitive_values(#[case] value: Value, #[case] expected: &str) {
        assert_eq!(render(value), expected);
    }

    #[rstest]
    #[case::empty_array(
        Value::Array(vec![]),
        "*empty*\n"
    )]
    #[case::primitive_array(
        Value::Array(vec![
            Value::String("a".into()),
            Value::String("b".into()),
        ]),
        "- a\n- b\n\n"
    )]
    fn test_array_values(#[case] value: Value, #[case] expected: &str) {
        assert_eq!(render(value), expected);
    }

    #[rstest]
    fn test_object_with_primitives() {
        let value = Value::Object(vec![
            ("name".into(), Value::String("Alice".into())),
            ("age".into(), Value::Integer(30)),
        ]);
        let expected = "\
| Key | Value |
|---|---|
| name | Alice |
| age | 30 |

";
        assert_eq!(render(value), expected);
    }

    #[rstest]
    fn test_object_with_nested_object() {
        let value = Value::Object(vec![
            ("name".into(), Value::String("Alice".into())),
            (
                "address".into(),
                Value::Object(vec![("city".into(), Value::String("Tokyo".into()))]),
            ),
        ]);
        let output = render(value);
        assert!(output.contains("| name | Alice |"));
        assert!(output.contains("# address"));
        assert!(output.contains("| city | Tokyo |"));
    }

    #[rstest]
    fn test_array_of_objects_as_table() {
        let value = Value::Array(vec![
            Value::Object(vec![
                ("id".into(), Value::Integer(1)),
                ("name".into(), Value::String("x".into())),
            ]),
            Value::Object(vec![
                ("id".into(), Value::Integer(2)),
                ("name".into(), Value::String("y".into())),
            ]),
        ]);
        let expected = "\
| id | name |
|---|---|
| 1 | x |
| 2 | y |

";
        assert_eq!(render(value), expected);
    }

    #[rstest]
    fn test_array_of_objects_with_nested_not_table() {
        let value = Value::Array(vec![Value::Object(vec![
            ("id".into(), Value::Integer(1)),
            ("tags".into(), Value::Array(vec![Value::String("a".into())])),
        ])]);
        let output = render(value);
        assert!(!output.starts_with("| id |"));
    }

    #[rstest]
    fn test_consecutive_primitives_grouped() {
        let value = Value::Object(vec![
            ("a".into(), Value::String("1".into())),
            ("b".into(), Value::String("2".into())),
            (
                "nested".into(),
                Value::Object(vec![("x".into(), Value::String("y".into()))]),
            ),
            ("c".into(), Value::String("3".into())),
        ]);
        let output = render(value);
        assert!(output.contains("| a | 1 |"));
        assert!(output.contains("| b | 2 |"));
        assert!(output.contains("# nested"));
        assert!(output.contains("| c | 3 |"));
    }

    #[rstest]
    fn test_pipe_escape_in_keys_and_values() {
        let value = Value::Object(vec![("a|b".into(), Value::String("c|d".into()))]);
        let output = render(value);
        assert!(output.contains("a\\|b"));
        assert!(output.contains("c\\|d"));
    }

    #[rstest]
    fn test_heading_depth_caps_at_6() {
        let mut v = Value::Object(vec![("g".into(), Value::String("leaf".into()))]);
        for key in ["f", "e", "d", "c", "b", "a"] {
            v = Value::Object(vec![(key.into(), v)]);
        }
        let output = render(v);
        assert!(output.contains("###### f") || output.contains("###### g"));
        assert!(!output.contains("#######"));
    }

    #[rstest]
    fn test_mixed_array_rendering() {
        let value = Value::Array(vec![
            Value::Integer(1),
            Value::Object(vec![("key".into(), Value::String("val".into()))]),
        ]);
        let output = render(value);
        assert!(output.contains("- 1"));
        assert!(output.contains("# 2"));
        assert!(output.contains("| key | val |"));
    }
}
