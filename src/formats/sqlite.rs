use std::io::Write;

use crate::converter::Converter;
use crate::document::{Block, Document};
use crate::error::{Error, Result};

pub struct SqliteConverter;

impl Converter for SqliteConverter {
    fn format_name(&self) -> &'static str {
        "sqlite"
    }

    fn convert(&self, input: &[u8], writer: &mut dyn Write) -> Result<()> {
        // Write input to a temporary file since rusqlite needs a file path
        let tmp = std::env::temp_dir().join(format!("mq-conv-{}.db", std::process::id()));
        std::fs::write(&tmp, input)?;

        let result = convert_db(&tmp, writer);

        let _ = std::fs::remove_file(&tmp);

        result
    }
}

fn convert_db(path: &std::path::Path, writer: &mut dyn Write) -> Result<()> {
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|e| Error::Conversion {
                format: "sqlite",
                message: e.to_string(),
            })?;

    // Get all table names
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .map_err(|e| Error::Conversion {
            format: "sqlite",
            message: e.to_string(),
        })?;

    let tables: Vec<String> = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| Error::Conversion {
            format: "sqlite",
            message: e.to_string(),
        })?
        .filter_map(|r| r.ok())
        .collect();

    let mut doc = Document::new();
    doc.push(Block::heading(1, "Database"));
    doc.push(Block::field("Tables", tables.len()));

    for table in &tables {
        doc.push(Block::heading(2, table));
        let quoted = table.replace('"', "\"\"");

        let mut col_stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{quoted}\")"))
            .map_err(|e| Error::Conversion {
                format: "sqlite",
                message: e.to_string(),
            })?;

        let columns: Vec<(String, String, bool)> = col_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, bool>(5)?,
                ))
            })
            .map_err(|e| Error::Conversion {
                format: "sqlite",
                message: e.to_string(),
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut schema = vec![["Column", "Type", "PK"].map(String::from).to_vec()];
        for (name, dtype, pk) in &columns {
            let pk_mark = if *pk { "yes" } else { "" };
            schema.push(vec![name.clone(), dtype.clone(), pk_mark.to_string()]);
        }
        doc.push(Block::text_table(&schema));

        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM \"{quoted}\""), [], |row| {
                row.get(0)
            })
            .unwrap_or(0);
        doc.push(Block::field("Rows", count));

        if count > 0 && !columns.is_empty() {
            let mut preview = vec![
                columns
                    .iter()
                    .map(|(n, _, _)| n.clone())
                    .collect::<Vec<_>>(),
            ];

            let mut data_stmt = conn
                .prepare(&format!("SELECT * FROM \"{quoted}\" LIMIT 10"))
                .map_err(|e| Error::Conversion {
                    format: "sqlite",
                    message: e.to_string(),
                })?;
            let mut rows = data_stmt.query([]).map_err(|e| Error::Conversion {
                format: "sqlite",
                message: e.to_string(),
            })?;
            while let Some(row) = rows.next().map_err(|e| Error::Conversion {
                format: "sqlite",
                message: e.to_string(),
            })? {
                preview.push(
                    (0..columns.len())
                        .map(|i| {
                            row.get::<_, rusqlite::types::Value>(i)
                                .map(|v| match v {
                                    rusqlite::types::Value::Null => "NULL".to_string(),
                                    rusqlite::types::Value::Integer(n) => n.to_string(),
                                    rusqlite::types::Value::Real(f) => f.to_string(),
                                    rusqlite::types::Value::Text(s) => s,
                                    rusqlite::types::Value::Blob(b) => {
                                        format!("(BLOB, {} bytes)", b.len())
                                    }
                                })
                                .unwrap_or_default()
                        })
                        .collect(),
                );
            }
            doc.push(Block::text_table(&preview));

            if count > 10 {
                doc.push(Block::emphasized(format!("Showing 10 of {count} rows")));
            }
        }
    }

    doc.write_markdown(writer)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Vec<u8> {
        let path = std::env::temp_dir().join(format!("mq-conv-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE t (id INTEGER PRIMARY KEY, note TEXT);
                 INSERT INTO t VALUES (1, 'a|b'), (2, NULL);
                 CREATE TABLE empty (x INTEGER);",
            )
            .unwrap();
            for n in 3..=13 {
                conn.execute("INSERT INTO t VALUES (?1, 'x')", [n]).unwrap();
            }
        }
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        bytes
    }

    #[test]
    fn describes_tables_schema_and_a_preview() {
        let mut out = Vec::new();
        SqliteConverter.convert(&database(), &mut out).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with("# Database\n\n**Tables**: 2\n\n"), "{out}");
        assert!(out.contains("## empty\n\n"), "{out}");
        assert!(out.contains("| id     | INTEGER | yes |"), "{out}");
        assert!(out.contains("**Rows**: 13"), "{out}");
        assert!(out.contains("a\\|b"), "{out}");
        assert!(out.contains("NULL"), "{out}");
        assert!(out.ends_with("*Showing 10 of 13 rows*\n"), "{out}");
    }
}
