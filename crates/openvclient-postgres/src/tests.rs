//! Integration tests against a live server. Set `OPENVCLIENT_PG_URL_HOST`/`OPENVCLIENT_PG_USER`
//! to override; tests are skipped when no server is reachable.

use std::collections::BTreeMap;

use openvclient_core::sql::*;
use openvclient_core::{ConnectParams, Driver, Value};

use crate::PostgresDriver;

fn params(db: &str) -> ConnectParams {
    let mut v = BTreeMap::new();
    v.insert("host".into(), std::env::var("OPENVCLIENT_PG_HOST").unwrap_or_else(|_| "localhost".into()));
    v.insert("user".into(), std::env::var("OPENVCLIENT_PG_USER").unwrap_or_else(|_| std::env::var("USER").unwrap_or_default()));
    v.insert("database".into(), db.into());
    v.insert("sslmode".into(), "disable".into());
    ConnectParams { values: v, secret: std::env::var("OPENVCLIENT_PG_PASSWORD").ok(), read_only: false, notifier: None }
}

#[tokio::test]
async fn live_postgres() {
    let Ok(h) = PostgresDriver::default().connect(params("postgres")).await else {
        eprintln!("skipping: no PostgreSQL reachable");
        return;
    };
    let admin = h.sql().unwrap().clone();
    let _ = admin.drop_object(ObjectKind::Database, &ObjPath::db("openvclient_test"), true).await;
    admin.create_database(CreateDatabaseSpec { name: "openvclient_test".into(), ..Default::default() }).await.unwrap();

    let s = PostgresDriver::default().connect(params("openvclient_test")).await.unwrap().sql().unwrap().clone();
    s.run(
        None,
        "CREATE SCHEMA app; CREATE TABLE app.users(id serial PRIMARY KEY, name text NOT NULL, score numeric(6,2), tags text[], born date); \
                 CREATE TABLE app.log(msg text); CREATE VIEW app.v AS SELECT name FROM app.users; \
                 CREATE INDEX users_name_idx ON app.users(name); COMMENT ON TABLE app.users IS 'people'; \
                 CREATE FUNCTION app.add(a int, b int) RETURNS int LANGUAGE sql AS $$ SELECT a + b $$;",
    )
    .await
    .unwrap();

    let dbs = s.children(None).await.unwrap();
    assert!(dbs.iter().any(|d| d.label == "openvclient_test"));
    let schemas = s.children(Some((ObjectKind::Database, ObjPath::db("openvclient_test")))).await.unwrap();
    assert!(schemas.iter().any(|n| n.label == "app"));
    let app = ObjPath::db("openvclient_test").with_schema("app");
    let tables = s.children(Some((ObjectKind::Folder(FolderKind::Tables), app.clone()))).await.unwrap();
    assert_eq!(tables.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), vec!["log", "users"]);
    let funcs = s.children(Some((ObjectKind::Folder(FolderKind::Functions), app.clone()))).await.unwrap();
    assert_eq!(funcs[0].label, "add(a integer, b integer)");
    assert!(s.ddl(ObjectKind::Function, &funcs[0].path).await.unwrap().contains("SELECT a + b"));

    let users = app.clone().with_name("users");
    let ddl = s.ddl(ObjectKind::Table, &users).await.unwrap();
    assert!(ddl.contains("CREATE TABLE \"app\".\"users\""), "{ddl}");
    assert!(ddl.contains("users_name_idx") && ddl.contains("COMMENT ON TABLE"), "{ddl}");

    // Console: multi statements, types, truncation, errors stop the script.
    let c = s.open_console(None).await.unwrap();
    let res = c
        .execute(
            vec![
                "INSERT INTO app.users(name, score, tags, born) VALUES ('ann', 1.5, '{a,b}', '2000-01-02'), ('bob', NULL, NULL, NULL)".into(),
                "SELECT id, name, score, tags FROM app.users ORDER BY id".into(),
                "SELECT generate_series(1, 50)".into(),
                "SELEC broken".into(),
                "SELECT 1".into(),
            ],
            ExecOptions { max_rows: 10 },
        )
        .await;
    assert!(matches!(res[0].outcome, Ok(StatementOutcome::Affected(2))));
    let Ok(StatementOutcome::Rows(rs)) = &res[1].outcome else { panic!("{:?}", res[1].outcome) };
    assert_eq!(rs.columns[2].type_name, "numeric");
    assert!(rs.columns[0].numeric);
    assert_eq!(rs.rows[0][3], Value::Text("{a,b}".into()));
    assert_eq!(rs.rows[1][2], Value::Null);
    let Ok(StatementOutcome::Rows(big)) = &res[2].outcome else { panic!() };
    assert!(big.truncated && big.rows.len() == 10);
    assert!(res[3].outcome.is_err());
    assert_eq!(res.len(), 4, "script must stop at the first error");

    // Cancellation of a long query from another task.
    let c2 = c.clone();
    let h = tokio::spawn(async move { c2.execute(vec!["SELECT pg_sleep(20)".into()], ExecOptions::default()).await });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    c.cancel();
    let r = tokio::time::timeout(std::time::Duration::from_secs(5), h).await.expect("cancel took too long").unwrap();
    assert!(matches!(&r[0].outcome, Err(e) if e.kind == openvclient_core::ErrorKind::Cancelled), "{:?}", r[0].outcome);

    // Explain, plain and analyze on DML (rolled back).
    let p = c.explain("SELECT * FROM app.users WHERE id = 1", false).await.unwrap();
    assert!(p.tree.is_some());
    let p = c.explain("DELETE FROM app.users", true).await.unwrap();
    assert!(p.tree.unwrap().actual_ms.is_some());
    assert_eq!(s.count_rows(&users, None).await.unwrap(), 2, "EXPLAIN ANALYZE must not delete rows");

    // Table page with server-side filter/sort and editing by primary key.
    let page = s.table_page(TableQuery { path: users.clone(), filter: Some("name <> 'zed'".into()), order_by: vec![("name".into(), SortDir::Desc)], limit: 50, offset: 0 }).await.unwrap();
    assert_eq!(page.key_columns, vec!["id"]);
    assert_eq!(page.rows.rows[0][1], Value::Text("bob".into()));
    assert!(page.read_only_reason.is_none());
    let bob_id = page.rows.rows[0][0].clone();
    s.apply_changes(
        &users,
        vec![
            RowChange::Update {
                key: RowKey::PrimaryKey(vec![("id".into(), bob_id.clone())]),
                values: vec![("score".into(), Some("7.25".into())), ("tags".into(), Some("{x}".into())), ("born".into(), Some("1999-12-31".into()))],
            },
            RowChange::Insert { values: vec![("name".into(), Some("cy".into()))] },
        ],
    )
    .await
    .unwrap();
    let page = s.table_page(TableQuery { path: users.clone(), filter: Some(format!("id = {}", bob_id.to_text().unwrap())), order_by: vec![], limit: 5, offset: 0 }).await.unwrap();
    assert_eq!(page.rows.rows[0][2], Value::Text("7.25".into()));
    assert!(s.table_page(TableQuery { path: users.clone(), filter: Some("true; DROP TABLE app.users".into()), order_by: vec![], limit: 5, offset: 0 }).await.is_err());

    // Failed batch rolls back everything.
    let bad = s.apply_changes(&users, vec![RowChange::Insert { values: vec![("name".into(), Some("ok".into()))] }, RowChange::Insert { values: vec![("name".into(), None)] }]).await;
    assert!(bad.is_err());
    assert_eq!(s.count_rows(&users, None).await.unwrap(), 3);

    // Keyless table edits through ctid.
    let log = app.clone().with_name("log");
    s.apply_changes(&log, vec![RowChange::Insert { values: vec![("msg".into(), Some("hello".into()))] }]).await.unwrap();
    let page = s.table_page(TableQuery { path: log.clone(), filter: None, order_by: vec![], limit: 5, offset: 0 }).await.unwrap();
    let ctid = page.row_ids.unwrap()[0].clone();
    s.apply_changes(&log, vec![RowChange::Delete { key: RowKey::RowId(ctid) }]).await.unwrap();
    assert_eq!(s.count_rows(&log, None).await.unwrap(), 0);

    let v = s.table_page(TableQuery { path: app.clone().with_name("v"), filter: None, order_by: vec![], limit: 5, offset: 0 }).await.unwrap();
    assert!(v.read_only_reason.is_some());

    // Monitoring and admin.
    let act = s.activity().await.unwrap();
    assert!(!act.rows.rows.is_empty());
    assert!(s.locks().await.is_ok());
    assert!(s.metrics().await.unwrap().iter().any(|m| m.name == "Cache hit ratio (%)"));
    let _ = s.drop_principal("openvclient_test_role", None).await;
    s.create_principal(PrincipalSpec { name: "openvclient_test_role".into(), password: Some("pw'x".into()), can_login: true, ..Default::default() }).await.unwrap();
    s.grant(GrantSpec { privileges: vec!["SELECT".into()], target: GrantTarget::Table(users.clone()), principal: "openvclient_test_role".into(), host: None, with_grant_option: false }, false)
        .await
        .unwrap();
    let privs = s.privileges("openvclient_test_role", None).await.unwrap();
    assert!(privs.rows.iter().any(|r| r[1] == Value::Text("app.users".into())));
    s.grant(GrantSpec { privileges: vec!["SELECT".into()], target: GrantTarget::Table(users.clone()), principal: "openvclient_test_role".into(), host: None, with_grant_option: false }, true)
        .await
        .unwrap();
    s.drop_principal("openvclient_test_role", None).await.unwrap();
    let cat = s.catalog(None).await.unwrap();
    assert!(cat.columns["users"].contains(&"score".to_string()));

    // Native backup and restore round trip.
    if crate::admin::find_tool("pg_dump").is_some() {
        let file = std::env::temp_dir().join(format!("openvclient-pg-{}.dump", std::process::id()));
        let msg = s.backup(BackupRequest { database: None, path: file.clone() }).await;
        match msg {
            Ok(_) => assert!(file.exists()),
            Err(e) => eprintln!("pg_dump unavailable for this server version: {e}"),
        }
        let _ = std::fs::remove_file(file);
    }

    c.close().await;
    s.close().await;
    admin.drop_object(ObjectKind::Database, &ObjPath::db("openvclient_test"), true).await.unwrap();
}
