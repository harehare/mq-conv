use std::io::Write;

use crate::converter::Converter;
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
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
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

    writeln!(writer, "# Database")?;
    writeln!(writer)?;
    writeln!(writer, "**Tables**: {}", tables.len())?;
    writeln!(writer)?;

    for (idx, table) in tables.iter().enumerate() {
        if idx > 0 {
            writeln!(writer)?;
        }
        writeln!(writer, "## {table}")?;
        writeln!(writer)?;

        // Get column info
        let mut col_stmt = conn
            .prepare(&format!("PRAGMA table_info(\"{}\")", table.replace('"', "\"\"")))
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

        // Schema
        writeln!(writer, "| Column | Type | PK |")?;
        writeln!(writer, "|--------|------|----|")?;
        for (name, dtype, pk) in &columns {
            let pk_mark = if *pk { "yes" } else { "" };
            writeln!(writer, "| {name} | {dtype} | {pk_mark} |")?;
        }
        writeln!(writer)?;

        // Row count
        let count: i64 = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM \"{}\"", table.replace('"', "\"\"")),
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        writeln!(writer, "**Rows**: {count}")?;

        // Preview first 10 rows
        if count > 0 && !columns.is_empty() {
            writeln!(writer)?;

            let col_names: Vec<&str> = columns.iter().map(|(n, _, _)| n.as_str()).collect();

            // Header
            write!(writer, "|")?;
            for name in &col_names {
                write!(writer, " {name} |")?;
            }
            writeln!(writer)?;

            // Separator
            write!(writer, "|")?;
            for _ in &col_names {
                write!(writer, "---|")?;
            }
            writeln!(writer)?;

            // Data (limit to 10 rows)
            let query = format!(
                "SELECT * FROM \"{}\" LIMIT 10",
                table.replace('"', "\"\"")
            );
            let mut data_stmt = conn.prepare(&query).map_err(|e| Error::Conversion {
                format: "sqlite",
                message: e.to_string(),
            })?;

            let col_count = columns.len();
            let mut rows = data_stmt.query([]).map_err(|e| Error::Conversion {
                format: "sqlite",
                message: e.to_string(),
            })?;

            while let Some(row) = rows.next().map_err(|e| Error::Conversion {
                format: "sqlite",
                message: e.to_string(),
            })? {
                write!(writer, "|")?;
                for i in 0..col_count {
                    let val: String = row
                        .get::<_, rusqlite::types::Value>(i)
                        .map(|v| match v {
                            rusqlite::types::Value::Null => "NULL".to_string(),
                            rusqlite::types::Value::Integer(n) => n.to_string(),
                            rusqlite::types::Value::Real(f) => f.to_string(),
                            rusqlite::types::Value::Text(s) => s.replace('|', "\\|"),
                            rusqlite::types::Value::Blob(b) => format!("[BLOB {} bytes]", b.len()),
                        })
                        .unwrap_or_default();
                    write!(writer, " {val} |")?;
                }
                writeln!(writer)?;
            }

            if count > 10 {
                writeln!(writer)?;
                writeln!(writer, "*Showing 10 of {count} rows*")?;
            }
        }
    }

    Ok(())
}
