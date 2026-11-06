//! Contract for Redis-protocol key-value stores (Redis, Valkey, KeyDB…).

use async_trait::async_trait;

use crate::{Result, ServiceSession};

#[derive(Clone, Debug)]
pub struct KeyEntry {
    pub key: String,
    pub key_type: String,
    /// Seconds; `None` = no expiry.
    pub ttl: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct ScanPage {
    pub cursor: u64,
    pub keys: Vec<KeyEntry>,
}

#[derive(Clone, Debug)]
pub enum KvValue {
    String(Vec<u8>),
    Hash(Vec<(String, String)>),
    List(Vec<String>),
    Set(Vec<String>),
    ZSet(Vec<(String, f64)>),
    /// Stream entries: (id, fields)
    Stream(Vec<(String, Vec<(String, String)>)>),
    /// Types we can show but not edit (modules, JSON…): rendered by the server.
    Other {
        type_name: String,
        rendered: String,
    },
}

#[derive(Clone, Debug)]
pub struct KeyDetail {
    pub key: String,
    pub key_type: String,
    pub ttl: Option<i64>,
    /// Element count (strings: byte length).
    pub length: u64,
    /// `MEMORY USAGE` in bytes, when the server supports it.
    pub memory: Option<u64>,
    pub encoding: Option<String>,
    pub value: KvValue,
    /// Only the first `limit` elements were loaded.
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub enum KvEdit {
    SetString { value: String },
    HashSet { field: String, value: String },
    HashDelete { field: String },
    ListPush { value: String, head: bool },
    ListSet { index: i64, value: String },
    ListRemove { value: String },
    SetAdd { member: String },
    SetRemove { member: String },
    ZAdd { member: String, score: f64 },
    ZRemove { member: String },
    StreamAdd { fields: Vec<(String, String)> },
    StreamDelete { id: String },
}

#[derive(Clone, Debug)]
pub struct KvDatabase {
    pub index: u32,
    pub keys: u64,
    pub expires: u64,
}

#[async_trait]
pub trait KeyValueSession: ServiceSession {
    async fn databases(&self) -> Result<Vec<KvDatabase>>;
    async fn scan(&self, db: u32, cursor: u64, pattern: &str, count: u32, type_filter: Option<&str>) -> Result<ScanPage>;
    async fn key_detail(&self, db: u32, key: &str, limit: usize) -> Result<KeyDetail>;
    async fn edit(&self, db: u32, key: &str, edit: KvEdit) -> Result<()>;
    async fn create_key(&self, db: u32, key: &str, key_type: &str, first: Vec<String>, ttl: Option<i64>) -> Result<()>;
    async fn delete_keys(&self, db: u32, keys: Vec<String>) -> Result<u64>;
    async fn rename(&self, db: u32, from: &str, to: &str) -> Result<()>;
    /// `None` removes the expiry.
    async fn set_ttl(&self, db: u32, key: &str, ttl: Option<i64>) -> Result<()>;
    /// Run a raw command line (quotes honoured); returns a printable reply.
    async fn command(&self, db: u32, line: &str) -> Result<String>;
    /// `INFO` sections as (section, key, value).
    async fn info(&self) -> Result<Vec<(String, String, String)>>;
    /// `CLIENT LIST` parsed into rows of key=value pairs.
    async fn clients(&self) -> Result<Vec<Vec<(String, String)>>>;
    async fn kill_client(&self, id: &str) -> Result<()>;
    async fn slowlog(&self, count: u32) -> Result<Vec<(i64, i64, u64, String)>>;
}

/// Split a command line into arguments, honouring single and double quotes
/// and backslash escapes inside double quotes (as `redis-cli` does).
pub fn split_command_line(line: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_arg = false;
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_arg = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some('n') => cur.push('\n'),
                            Some('t') => cur.push('\t'),
                            Some('r') => cur.push('\r'),
                            Some(o) => cur.push(o),
                            None => return Err(crate::Error::invalid("Unterminated escape")),
                        },
                        Some(o) => cur.push(o),
                        None => return Err(crate::Error::invalid("Unbalanced quotes")),
                    }
                }
            }
            '\'' => {
                in_arg = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(o) => cur.push(o),
                        None => return Err(crate::Error::invalid("Unbalanced quotes")),
                    }
                }
            }
            c if c.is_whitespace() => {
                if in_arg {
                    args.push(std::mem::take(&mut cur));
                    in_arg = false;
                }
            }
            o => {
                in_arg = true;
                cur.push(o);
            }
        }
    }
    if in_arg {
        args.push(cur);
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    #[test]
    fn split() {
        let a = super::split_command_line(r#"SET "my key" 'a b' "x\"y" z"#).unwrap();
        assert_eq!(a, vec!["SET", "my key", "a b", "x\"y", "z"]);
    }
}
