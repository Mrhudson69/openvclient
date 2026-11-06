//! Result export (CSV, TSV, JSON, SQL INSERT, Markdown) and CSV import parsing.

use openvclient_core::sqltext::Dialect;
use openvclient_core::{ResultSet, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Csv,
    Tsv,
    Json,
    SqlInsert,
    Markdown,
}

impl Format {
    pub const ALL: [Format; 5] = [Format::Csv, Format::Tsv, Format::Json, Format::SqlInsert, Format::Markdown];
    pub fn label(self) -> &'static str {
        match self {
            Format::Csv => "CSV",
            Format::Tsv => "TSV",
            Format::Json => "JSON",
            Format::SqlInsert => "SQL INSERT",
            Format::Markdown => "Markdown",
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Tsv => "tsv",
            Format::Json => "json",
            Format::SqlInsert => "sql",
            Format::Markdown => "md",
        }
    }
}

/// Render `rows` (indexes into `rs.rows`) of a result.
pub fn render(rs: &ResultSet, rows: &[usize], cols: Option<&[usize]>, fmt: Format, dialect: Option<Dialect>, table: &str) -> String {
    let all_cols: Vec<usize> = (0..rs.columns.len()).collect();
    let cols = cols.unwrap_or(&all_cols);
    let names: Vec<&str> = cols.iter().map(|&c| rs.columns[c].name.as_str()).collect();
    let cell = |r: usize, c: usize| &rs.rows[r][c];
    match fmt {
        Format::Csv | Format::Tsv => {
            let mut w = csv::WriterBuilder::new().delimiter(if fmt == Format::Csv { b',' } else { b'\t' }).from_writer(Vec::new());
            let _ = w.write_record(&names);
            for &r in rows {
                let rec: Vec<String> = cols.iter().map(|&c| cell(r, c).to_text().unwrap_or_default()).collect();
                let _ = w.write_record(&rec);
            }
            String::from_utf8(w.into_inner().unwrap_or_default()).unwrap_or_default()
        }
        Format::Json => {
            let arr: Vec<serde_json::Value> = rows
                .iter()
                .map(|&r| {
                    let mut m = serde_json::Map::new();
                    for (i, &c) in cols.iter().enumerate() {
                        m.insert(names[i].to_string(), json_value(cell(r, c), rs.columns[c].numeric));
                    }
                    serde_json::Value::Object(m)
                })
                .collect();
            serde_json::to_string_pretty(&arr).unwrap_or_default()
        }
        Format::SqlInsert => {
            let d = dialect.unwrap_or(Dialect::Postgres);
            let col_list = names.iter().map(|n| d.quote_ident(n)).collect::<Vec<_>>().join(", ");
            let target = if table.is_empty() { d.quote_ident("table_name") } else { table.to_string() };
            let mut out = String::new();
            for &r in rows {
                let vals: Vec<String> = cols.iter().map(|&c| sql_literal(d, cell(r, c), rs.columns[c].numeric)).collect();
                out.push_str(&format!("INSERT INTO {target} ({col_list}) VALUES ({});\n", vals.join(", ")));
            }
            out
        }
        Format::Markdown => {
            let esc = |s: &str| s.replace('|', "\\|").replace('\n', " ");
            let mut out = format!("| {} |\n|{}|\n", names.iter().map(|n| esc(n)).collect::<Vec<_>>().join(" | "), vec![" --- "; names.len()].join("|"));
            for &r in rows {
                let vals: Vec<String> = cols.iter().map(|&c| esc(&cell(r, c).display())).collect();
                out.push_str(&format!("| {} |\n", vals.join(" | ")));
            }
            out
        }
    }
}

fn json_value(v: &Value, numeric: bool) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => (*i).into(),
        Value::UInt(u) => (*u).into(),
        Value::Float(f) => serde_json::Number::from_f64(*f).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
        Value::Text(s) if numeric => s.parse::<serde_json::Number>().map(serde_json::Value::Number).unwrap_or_else(|_| s.clone().into()),
        Value::Text(s) => s.clone().into(),
        Value::Bytes(b) => openvclient_core::hex_full(b).into(),
    }
}

fn sql_literal(d: Dialect, v: &Value, numeric: bool) -> String {
    match v {
        Value::Null => "NULL".into(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Int(_) | Value::UInt(_) | Value::Float(_) => v.to_text().unwrap_or_default(),
        Value::Text(s) if numeric && s.parse::<f64>().is_ok() => s.clone(),
        Value::Text(s) => d.quote_literal(s),
        Value::Bytes(b) => match d {
            Dialect::Postgres => format!("'\\x{}'::bytea", &openvclient_core::hex_full(b)[2..]),
            Dialect::MySql | Dialect::SqlServer => openvclient_core::hex_full(b),
            Dialect::Sqlite => format!("X'{}'", &openvclient_core::hex_full(b)[2..]),
        },
    }
}

/// Parsed CSV: header plus records. Empty cells stay empty strings; the
/// caller decides whether they mean NULL.
pub struct CsvData {
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

pub fn read_csv(path: &std::path::Path) -> Result<CsvData, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let delim = if path.extension().is_some_and(|e| e == "tsv") { b'\t' } else { b',' };
    let mut r = csv::ReaderBuilder::new().delimiter(delim).flexible(false).from_reader(bytes.as_slice());
    let header: Vec<String> = r.headers().map_err(|e| e.to_string())?.iter().map(|h| h.trim().to_string()).collect();
    let mut rows = Vec::new();
    for rec in r.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        rows.push(rec.iter().map(str::to_string).collect());
    }
    Ok(CsvData { header, rows })
}

#[cfg(test)]
mod tests {
    use super::*;
    use openvclient_core::ColumnMeta;

    fn rs() -> ResultSet {
        ResultSet {
            columns: vec![ColumnMeta::new("id", "int"), ColumnMeta::new("name", "text")],
            rows: vec![vec![Value::Int(1), Value::Text("a,\"b\"".into())], vec![Value::Int(2), Value::Null]],
            truncated: false,
        }
    }

    #[test]
    fn csv_and_sql() {
        let r = rs();
        let csv = render(&r, &[0, 1], None, Format::Csv, None, "");
        assert_eq!(csv, "id,name\n1,\"a,\"\"b\"\"\"\n2,\n");
        let sql = render(&r, &[0, 1], None, Format::SqlInsert, Some(Dialect::MySql), "`t`");
        assert!(sql.contains("VALUES (1, 'a,\"b\"')") && sql.contains("VALUES (2, NULL)"), "{sql}");
        let json = render(&r, &[1], None, Format::Json, None, "");
        assert!(json.contains("\"name\": null"));
    }
}
