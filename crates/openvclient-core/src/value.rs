use std::borrow::Cow;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// A single cell as returned by a driver. Drivers keep the engine's own text
/// rendering where the wire protocol provides one (PostgreSQL text format,
/// MySQL decimals) so nothing is lost to lossy conversions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float(f64),
    Text(String),
    Bytes(Vec<u8>),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Value::Int(_) | Value::UInt(_) | Value::Float(_))
    }

    /// Text shown in a grid cell; long values are cut so layout stays fast.
    pub fn display(&self) -> Cow<'_, str> {
        const MAX: usize = 512;
        match self {
            Value::Null => Cow::Borrowed("NULL"),
            Value::Bool(b) => Cow::Borrowed(if *b { "true" } else { "false" }),
            Value::Int(i) => Cow::Owned(i.to_string()),
            Value::UInt(u) => Cow::Owned(u.to_string()),
            Value::Float(f) => Cow::Owned(f.to_string()),
            Value::Text(s) => {
                let first_line = s.lines().next().unwrap_or("");
                if first_line.len() == s.len() && s.len() <= MAX {
                    Cow::Borrowed(s.as_str())
                } else {
                    let mut cut = first_line.chars().take(MAX).collect::<String>();
                    cut.push('…');
                    Cow::Owned(cut)
                }
            }
            Value::Bytes(b) => Cow::Owned(hex_preview(b, 32)),
        }
    }

    /// Full textual form used for copying, editing and export. `None` for NULL.
    pub fn to_text(&self) -> Option<String> {
        match self {
            Value::Null => None,
            Value::Bool(b) => Some(b.to_string()),
            Value::Int(i) => Some(i.to_string()),
            Value::UInt(u) => Some(u.to_string()),
            Value::Float(f) => Some(f.to_string()),
            Value::Text(s) => Some(s.clone()),
            Value::Bytes(b) => Some(hex_full(b)),
        }
    }

    /// Ordering used by client-side sorting of a result grid.
    pub fn cmp_for_sort(&self, other: &Value) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        use Value::*;
        match (self, other) {
            (Null, Null) => Equal,
            (Null, _) => Less,
            (_, Null) => Greater,
            (a, b) if a.is_numeric() && b.is_numeric() => {
                a.as_f64().partial_cmp(&b.as_f64()).unwrap_or(Equal)
            }
            (Text(a), Text(b)) => match (a.parse::<f64>(), b.parse::<f64>()) {
                (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Equal),
                _ => a.cmp(b),
            },
            (a, b) => a.display().cmp(&b.display()),
        }
    }

    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Int(i) => *i as f64,
            Value::UInt(u) => *u as f64,
            Value::Float(f) => *f,
            Value::Bool(b) => *b as i64 as f64,
            Value::Text(s) => s.parse().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }
}

pub fn hex_full(b: &[u8]) -> String {
    let mut s = String::with_capacity(2 + b.len() * 2);
    s.push_str("0x");
    for byte in b {
        let _ = write!(s, "{byte:02x}");
    }
    s
}

fn hex_preview(b: &[u8], max: usize) -> String {
    let mut s = hex_full(&b[..b.len().min(max)]);
    if b.len() > max {
        let _ = write!(s, "… ({} bytes)", b.len());
    }
    s
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnMeta {
    pub name: String,
    /// Engine type name, e.g. `int4`, `varchar(255)`. Empty when unknown.
    pub type_name: String,
    /// Right-align and sort numerically.
    pub numeric: bool,
}

impl ColumnMeta {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        let type_name = type_name.into();
        let numeric = is_numeric_type(&type_name);
        Self { name: name.into(), type_name, numeric }
    }
}

pub fn is_numeric_type(t: &str) -> bool {
    let t = t.to_ascii_lowercase();
    let base = t.split(['(', ' ']).next().unwrap_or("");
    matches!(
        base,
        "int" | "int2" | "int4" | "int8" | "integer" | "smallint" | "bigint" | "tinyint" | "mediumint"
            | "serial" | "bigserial" | "smallserial" | "numeric" | "decimal" | "real" | "float"
            | "float4" | "float8" | "double" | "money" | "oid" | "year" | "bit" | "newdecimal"
            | "longlong" | "long" | "short" | "tiny" | "int24"
            // ClickHouse
            | "uint8" | "uint16" | "uint32" | "uint64" | "uint128" | "uint256" | "int16" | "int32" | "int64"
            | "int128" | "int256" | "float32" | "float64" | "decimal32" | "decimal64" | "decimal128" | "decimal256"
    )
}

/// Rows returned by one statement or one table page.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResultSet {
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Vec<Value>>,
    /// More rows existed than were fetched (row limit reached).
    pub truncated: bool,
}

impl ResultSet {
    pub fn new(columns: Vec<ColumnMeta>) -> Self {
        Self { columns, rows: Vec::new(), truncated: false }
    }

    /// Build a result from string pairs; used for engine metadata views.
    pub fn from_text_rows(columns: &[&str], rows: Vec<Vec<Option<String>>>) -> Self {
        Self {
            columns: columns.iter().map(|c| ColumnMeta::new(*c, "")).collect(),
            rows: rows
                .into_iter()
                .map(|r| r.into_iter().map(|v| v.map(Value::Text).unwrap_or(Value::Null)).collect())
                .collect(),
            truncated: false,
        }
    }

    pub fn column_index(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name.eq_ignore_ascii_case(name))
    }
}
