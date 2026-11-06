//! Integration test against a live server; skipped when none is reachable.

use std::collections::BTreeMap;

use openvclient_core::sql::*;
use openvclient_core::{ConnectParams, Driver, ErrorKind, Value};

use crate::MySqlDriver;

fn params(db: Option<&str>) -> ConnectParams {
    let mut v = BTreeMap::new();
    v.insert("host".into(), std::env::var("OPENVCLIENT_MY_HOST").unwrap_or_else(|_| "127.0.0.1".into()));
    v.insert("user".into(), std::env::var("OPENVCLIENT_MY_USER").unwrap_or_else(|_| "root".into()));
    if let Ok(port) = std::env::var("OPENVCLIENT_MY_PORT") {
        v.insert("port".into(), port);
    }
    if let Some(db) = db {
        v.insert("database".into(), db.into());
    }
    ConnectParams { values: v, secret: std::env::var("OPENVCLIENT_MY_PASSWORD").ok(), read_only: false, notifier: None }
}

#[tokio::test]
async fn live_mysql() {
    let admin = match MySqlDriver::default().connect(params(None)).await {
        Ok(h) => h.sql().unwrap().clone(),
        Err(e) => {
            eprintln!("skipping: no MySQL reachable ({e})");
            return;
        }
    };
    let _ = admin.drop_object(ObjectKind::Database, &ObjPath::db("openvclient_test"), false).await;
    admin.create_database(CreateDatabaseSpec { name: "openvclient_test".into(), encoding: Some("utf8mb4".into()), ..Default::default() }).await.unwrap();
    let s = MySqlDriver::default().connect(params(Some("openvclient_test"))).await.unwrap().sql().unwrap().clone();
    s.run(
        None,
        "CREATE TABLE users(id INT AUTO_INCREMENT PRIMARY KEY, name VARCHAR(40) NOT NULL, score DECIMAL(6,2), born DATE, seen DATETIME(3), data BLOB, flag TINYINT(1));\n\
         CREATE TABLE nokey(a TEXT);\n\
         CREATE VIEW v AS SELECT name FROM users;\n\
         CREATE INDEX users_name ON users(name);\n\
         DELIMITER //\nCREATE PROCEDURE p(IN x INT) BEGIN SELECT x + 1 AS y; IF x > 0 THEN SELECT 2; END IF; END //\nDELIMITER ;\n",
    )
    .await
    .unwrap();

    let dbs = s.children(None).await.unwrap();
    assert!(dbs.iter().any(|d| d.label == "openvclient_test"));
    let base = ObjPath::db("openvclient_test");
    let tables = s.children(Some((ObjectKind::Folder(FolderKind::Tables), base.clone()))).await.unwrap();
    assert_eq!(tables.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), vec!["nokey", "users"]);
    let procs = s.children(Some((ObjectKind::Folder(FolderKind::Procedures), base.clone()))).await.unwrap();
    assert_eq!(procs[0].label, "p");
    assert!(s.ddl(ObjectKind::Procedure, &procs[0].path).await.unwrap().contains("SELECT x + 1"));
    let users = base.clone().with_name("users");
    assert!(s.ddl(ObjectKind::Table, &users).await.unwrap().contains("CREATE TABLE `users`"));
    let idx = s.children(Some((ObjectKind::Folder(FolderKind::Indexes), users.clone()))).await.unwrap();
    assert!(idx.iter().any(|i| i.label == "users_name"));

    let c = s.open_console(None).await.unwrap();
    let res = c
        .execute(
            vec![
                "INSERT INTO users(name, score, born, seen, data, flag) VALUES ('ann', 1.50, '2000-01-02', '2024-05-06 07:08:09.123', x'00ff', 1), ('bob', NULL, NULL, NULL, NULL, 0)".into(),
                "SELECT id, name, score, born, seen, data FROM users ORDER BY id".into(),
                "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 50) SELECT i FROM n".into(),
                "CALL p(1)".into(),
                "SELEC broken".into(),
                "SELECT 1".into(),
            ],
            ExecOptions { max_rows: 10 },
        )
        .await;
    assert!(matches!(res[0].outcome, Ok(StatementOutcome::Affected(2))), "{:?}", res[0].outcome);
    let Ok(StatementOutcome::Rows(rs)) = &res[1].outcome else { panic!("{:?}", res[1].outcome) };
    assert_eq!(rs.rows[0][2], Value::Text("1.50".into()));
    assert_eq!(rs.rows[0][3], Value::Text("2000-01-02".into()));
    assert_eq!(rs.rows[0][4], Value::Text("2024-05-06 07:08:09.123".into()));
    assert_eq!(rs.rows[0][5], Value::Bytes(vec![0, 255]));
    assert!(rs.columns[0].numeric);
    let Ok(StatementOutcome::Rows(big)) = &res[2].outcome else { panic!() };
    assert!(big.truncated && big.rows.len() == 10);
    assert!(matches!(&res[3].outcome, Ok(StatementOutcome::Rows(r)) if r.rows[0][0] == Value::Int(2) || r.rows[0][0] == Value::Text("2".into())), "{:?}", res[3].outcome);
    assert!(res[4].outcome.is_err());
    assert_eq!(res.len(), 5);

    // Cancel a long query.
    let c2 = c.clone();
    let h = tokio::spawn(async move { c2.execute(vec!["SELECT SLEEP(20)".into()], ExecOptions::default()).await });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    c.cancel();
    let r = tokio::time::timeout(std::time::Duration::from_secs(5), h).await.expect("cancel too slow").unwrap();
    // KILL QUERY makes SLEEP() return 1 instead of raising, or raises 1317.
    match &r[0].outcome {
        Ok(StatementOutcome::Rows(rows)) => assert_eq!(rows.rows[0][0].to_text().as_deref(), Some("1")),
        Err(e) => assert_eq!(e.kind, ErrorKind::Cancelled),
        o => panic!("{o:?}"),
    }

    let p = c.explain("SELECT * FROM users WHERE id = 1", false).await.unwrap();
    assert!(p.tree.is_some() || p.table.is_some());
    let p = c.explain("DELETE FROM users WHERE id > 0", true).await;
    eprintln!("explain analyze DML: {:?}", p.as_ref().map(|x| x.raw.clone()));
    assert_eq!(s.count_rows(&users, None).await.unwrap(), 2);

    let page = s.table_page(TableQuery { path: users.clone(), filter: Some("name <> 'zed'".into()), order_by: vec![("name".into(), SortDir::Desc)], limit: 50, offset: 0 }).await.unwrap();
    assert_eq!(page.key_columns, vec!["id"]);
    assert_eq!(page.rows.rows[0][1], Value::Text("bob".into()));
    assert!(page.rows.columns[2].type_name.starts_with("decimal"));
    let bob = page.rows.rows[0][0].clone();
    // Same value update must still count as found (CLIENT_FOUND_ROWS).
    s.apply_changes(
        &users,
        vec![
            RowChange::Update { key: RowKey::PrimaryKey(vec![("id".into(), bob.clone())]), values: vec![("name".into(), Some("bob".into())), ("score".into(), Some("7.25".into()))] },
            RowChange::Insert { values: vec![("name".into(), Some("cy".into()))] },
        ],
    )
    .await
    .unwrap();
    assert_eq!(s.count_rows(&users, Some("score = 7.25")).await.unwrap(), 1);
    assert!(s.table_page(TableQuery { path: users.clone(), filter: Some("1=1; DROP TABLE users".into()), order_by: vec![], limit: 5, offset: 0 }).await.is_err());
    assert_eq!(s.count_rows(&users, None).await.unwrap(), 3);
    let bad = s.apply_changes(&users, vec![RowChange::Insert { values: vec![("name".into(), Some("ok".into()))] }, RowChange::Insert { values: vec![("name".into(), None)] }]).await;
    assert!(bad.is_err());
    assert_eq!(s.count_rows(&users, None).await.unwrap(), 3, "failed batch must roll back");
    let nk = s.table_page(TableQuery { path: base.clone().with_name("nokey"), filter: None, order_by: vec![], limit: 5, offset: 0 }).await.unwrap();
    assert!(nk.read_only_reason.is_some());

    assert!(!s.activity().await.unwrap().rows.rows.is_empty());
    s.locks().await.unwrap();
    assert!(s.metrics().await.unwrap().iter().any(|m| m.name == "Buffer pool hit ratio (%)"));
    let _ = s.drop_principal("openvclient_test_user", Some("localhost")).await;
    s.create_principal(PrincipalSpec { name: "openvclient_test_user".into(), host: Some("localhost".into()), password: Some("Pw'x1!aa".into()), can_login: true, ..Default::default() }).await.unwrap();
    s.grant(
        GrantSpec { privileges: vec!["SELECT".into()], target: GrantTarget::Table(users.clone()), principal: "openvclient_test_user".into(), host: Some("localhost".into()), with_grant_option: false },
        false,
    )
    .await
    .unwrap();
    let g = s.privileges("openvclient_test_user", Some("localhost")).await.unwrap();
    assert!(g.rows.iter().any(|r| r[0].to_text().unwrap().contains("SELECT ON `openvclient_test`.`users`")), "{:?}", g.rows);
    s.drop_principal("openvclient_test_user", Some("localhost")).await.unwrap();
    assert!(!s.principals().await.unwrap().rows.is_empty());
    assert!(s.catalog(None).await.unwrap().columns["users"].contains(&"score".to_string()));

    if s.capabilities().backup {
        let f = std::env::temp_dir().join(format!("openvclient-my-{}.sql", std::process::id()));
        s.backup(BackupRequest { database: None, path: f.clone() }).await.unwrap();
        assert!(std::fs::read_to_string(&f).unwrap().contains("CREATE TABLE `users`"));
        let _ = std::fs::remove_file(f);
    }

    c.close().await;
    s.close().await;
    admin.drop_object(ObjectKind::Database, &ObjPath::db("openvclient_test"), false).await.unwrap();
}
