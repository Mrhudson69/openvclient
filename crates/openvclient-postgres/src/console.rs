//! Dedicated per-tab connection with server-side cancellation.

use std::time::Instant;

use async_trait::async_trait;
use openvclient_core::Result;
use openvclient_core::explain::{ExplainResult, parse_pg_json_plan};
use openvclient_core::sql::*;
use openvclient_core::sqltext::leading_keyword;
use tokio::sync::Mutex;
use tokio_postgres::{CancelToken, Client};

use crate::{PgTarget, pg_err, simple_rows};

pub struct PgConsole {
    client: Mutex<Client>,
    cancel: CancelToken,
    target: PgTarget,
    database: String,
    rt: tokio::runtime::Handle,
}

impl PgConsole {
    pub async fn open(target: PgTarget, database: String) -> Result<Self> {
        let client = target.connect_client(&database).await?;
        let cancel = client.cancel_token();
        Ok(Self { client: Mutex::new(client), cancel, target, database, rt: tokio::runtime::Handle::current() })
    }
}

/// Statements that return rows; we parse them first to learn column types.
fn returns_rows(kw: &str) -> bool {
    matches!(kw, "SELECT" | "WITH" | "VALUES" | "TABLE" | "SHOW")
}

#[async_trait]
impl SqlConsole for PgConsole {
    async fn execute(&self, statements: Vec<String>, opts: ExecOptions) -> Vec<StatementResult> {
        let client = self.client.lock().await;
        let mut out = Vec::new();
        for sql in statements {
            let t = Instant::now();
            let kw = leading_keyword(&sql);
            let types = if returns_rows(&kw) {
                match client.prepare(&sql).await {
                    Ok(stmt) => Some(stmt.columns().iter().map(|c| c.type_().name().to_string()).collect()),
                    Err(_) => None,
                }
            } else {
                None
            };
            let outcome = simple_rows(&client, &sql, opts.max_rows, types).await;
            let failed = outcome.is_err();
            out.push(StatementResult { sql, elapsed: t.elapsed(), outcome });
            if failed {
                break;
            }
        }
        out
    }

    async fn explain(&self, sql: &str, analyze: bool) -> Result<ExplainResult> {
        let client = self.client.lock().await;
        let kw = leading_keyword(sql);
        let opts = if analyze { "FORMAT JSON, ANALYZE, BUFFERS" } else { "FORMAT JSON" };
        let statement = format!("EXPLAIN ({opts}) {sql}");
        // ANALYZE executes the statement; keep data-changing ones side-effect free.
        let wrap = analyze && !matches!(kw.as_str(), "SELECT" | "WITH" | "VALUES" | "TABLE");
        if wrap {
            client.batch_execute("BEGIN").await.map_err(pg_err)?;
        }
        let res = client.simple_query(&statement).await.map_err(pg_err);
        if wrap {
            let _ = client.batch_execute("ROLLBACK").await;
        }
        let msgs = res?;
        let raw: String = msgs
            .iter()
            .filter_map(|m| match m {
                tokio_postgres::SimpleQueryMessage::Row(r) => r.get(0).map(str::to_owned),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        let (tree, mut summary) = match parse_pg_json_plan(&raw) {
            Some((t, s)) => (Some(t), s),
            None => (None, Vec::new()),
        };
        if wrap {
            summary.push(("Note".into(), "Statement ran inside a transaction that was rolled back".into()));
        }
        Ok(ExplainResult { statement, raw, tree, table: None, summary })
    }

    fn cancel(&self) {
        let token = self.cancel.clone();
        let tls = self.target.tls();
        self.rt.spawn(async move {
            match tls {
                Ok(tls) => {
                    if let Err(e) = token.cancel_query(tls).await {
                        tracing::warn!("cancel failed: {e}");
                    }
                }
                Err(e) => tracing::warn!("cancel failed: {e}"),
            }
        });
    }

    fn current_database(&self) -> Option<String> {
        Some(self.database.clone())
    }

    async fn close(&self) {}
}
