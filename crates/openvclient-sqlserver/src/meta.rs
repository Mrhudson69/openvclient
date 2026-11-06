//! Catalog browsing, DDL, paging, editing and metrics from sys.* views.

use openvclient_core::sql::*;
use openvclient_core::sqltext::split_statements;
use openvclient_core::{ColumnMeta, Error, Result};

use crate::{D, MsSession, ms_err, result, text_rows};

const SYSTEM_DBS: &[&str] = &["master", "model", "msdb", "tempdb"];

/// A WHERE filter must be one expression, not a second statement.
pub fn single_statement(filter: &str) -> Result<()> {
    if split_statements(D, &format!("SELECT 1 WHERE {filter}")).len() != 1 || filter.to_ascii_uppercase().contains("\nGO") {
        return Err(Error::invalid("The filter must be a single expression (no ';' or GO)"));
    }
    Ok(())
}

fn human_kb(kb: Option<&String>) -> String {
    kb.and_then(|v| v.parse::<f64>().ok()).filter(|v| *v > 0.0).map(|v| openvclient_core::containers::human_bytes((v * 1024.0) as u64)).unwrap_or_default()
}

pub async fn children(s: &MsSession, node: Option<(ObjectKind, ObjPath)>) -> Result<Vec<TreeNode>> {
    // Object Explorer layout, as in SQL Server Management Studio.
    let Some((kind, path)) = node else {
        return Ok(vec![
            TreeNode::folder(FolderKind::Databases, ObjPath::default()),
            TreeNode::folder(FolderKind::Security, ObjPath::default()).leaf().detail("logins"),
            TreeNode::folder(FolderKind::ActivityMonitor, ObjPath::default()).leaf(),
        ]);
    };
    let db = s.db_of(&path);
    match kind {
        ObjectKind::Folder(f @ (FolderKind::Databases | FolderKind::SystemDatabases)) => {
            let mut c = s.conn(Some("master")).await?;
            let rows = text_rows(
                &mut c,
                "SELECT d.name, d.state_desc, CAST(SUM(CAST(f.size AS bigint)) * 8 AS bigint), CAST(CASE WHEN d.database_id <= 4 THEN 1 ELSE 0 END AS int) \
                 FROM sys.databases d LEFT JOIN sys.master_files f ON f.database_id = d.database_id \
                 GROUP BY d.name, d.state_desc, d.database_id ORDER BY d.name",
                &[],
            )
            .await?;
            let want_system = f == FolderKind::SystemDatabases;
            let mut out: Vec<TreeNode> = if want_system { Vec::new() } else { vec![TreeNode::folder(FolderKind::SystemDatabases, ObjPath::default())] };
            for r in rows {
                if (r[3].as_deref() == Some("1")) != want_system {
                    continue;
                }
                let name = r[0].clone().unwrap_or_default();
                let state = r[1].clone().unwrap_or_default();
                let mut d = human_kb(r[2].as_ref());
                if state != "ONLINE" {
                    d = format!("{} {d}", state.to_lowercase());
                }
                let n = TreeNode::new(ObjectKind::Database, name.clone(), ObjPath::db(name)).detail(d.trim());
                out.push(if state == "ONLINE" { n } else { n.leaf() });
            }
            Ok(out)
        }
        ObjectKind::Database => Ok(vec![
            TreeNode::folder(FolderKind::Tables, ObjPath::db(&db)),
            TreeNode::folder(FolderKind::Views, ObjPath::db(&db)),
            TreeNode::folder(FolderKind::Programmability, ObjPath::db(&db)),
            TreeNode::new(ObjectKind::Schema, "Schemas", ObjPath::db(&db).with_sub("#schemas")),
        ]),
        ObjectKind::Schema if path.sub.as_deref() == Some("#schemas") => {
            let mut c = s.conn(Some(&db)).await?;
            let rows = text_rows(&mut c, "SELECT name FROM sys.schemas WHERE name NOT IN ('guest','INFORMATION_SCHEMA','sys') AND name NOT LIKE 'db[_]%' ORDER BY name", &[]).await?;
            Ok(rows.into_iter().filter_map(|r| r[0].clone()).map(|n| TreeNode::new(ObjectKind::Schema, n.clone(), ObjPath::db(&db).with_schema(n)).leaf()).collect())
        }
        ObjectKind::Folder(FolderKind::Programmability) => Ok(vec![
            TreeNode::folder(FolderKind::Procedures, ObjPath::db(&db)),
            TreeNode::folder(FolderKind::Functions, ObjPath::db(&db)),
            TreeNode::folder(FolderKind::Sequences, ObjPath::db(&db)),
        ]),
        ObjectKind::Folder(f @ (FolderKind::Tables | FolderKind::Views | FolderKind::Procedures | FolderKind::Functions | FolderKind::Sequences)) => {
            let (types, okind, leaf) = match f {
                FolderKind::Tables => ("'U'", ObjectKind::Table, false),
                FolderKind::Views => ("'V'", ObjectKind::View, false),
                FolderKind::Procedures => ("'P','PC'", ObjectKind::Procedure, true),
                FolderKind::Functions => ("'FN','IF','TF','FS','FT'", ObjectKind::Function, true),
                _ => ("'SO'", ObjectKind::Sequence, true),
            };
            // Folder under a schema lists that schema; under a database, all schemas.
            let schema_filter = if path.schema.is_some() { "AND o.schema_id = SCHEMA_ID(@P1)" } else { "AND @P1 = @P1" };
            let sql = format!(
                "SELECT SCHEMA_NAME(o.schema_id), o.name, CASE WHEN o.type = 'U' THEN (SELECT SUM(p.rows) FROM sys.partitions p WHERE p.object_id = o.object_id AND p.index_id IN (0,1)) END, \
                 CASE WHEN o.type = 'U' THEN (SELECT SUM(a.total_pages) * 8 FROM sys.partitions p JOIN sys.allocation_units a ON a.container_id = p.partition_id WHERE p.object_id = o.object_id) END \
                 FROM sys.objects o WHERE o.type IN ({types}) AND o.is_ms_shipped = 0 {schema_filter} ORDER BY 1, 2"
            );
            let schema = path.schema.clone().unwrap_or_default();
            let mut c = s.conn(Some(&db)).await?;
            let rows = text_rows(&mut c, &sql, &[&schema.as_str()]).await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let sch = r[0].clone().unwrap_or_default();
                    let name = r[1].clone().unwrap_or_default();
                    let mut parts = Vec::new();
                    if let Some(n) = r[2].as_ref() {
                        parts.push(format!("~{n} rows"));
                    }
                    let kb = human_kb(r[3].as_ref());
                    if !kb.is_empty() {
                        parts.push(kb);
                    }
                    let n = TreeNode::new(okind, format!("{sch}.{name}"), ObjPath::db(&db).with_schema(&sch).with_name(name)).detail(parts.join(" · "));
                    if leaf { n.leaf() } else { n }
                })
                .collect())
        }
        ObjectKind::Table | ObjectKind::View => {
            let mut v = vec![TreeNode::folder(FolderKind::Columns, path.clone())];
            if kind == ObjectKind::Table {
                v.push(TreeNode::folder(FolderKind::Constraints, path.clone()));
                v.push(TreeNode::folder(FolderKind::Triggers, path.clone()));
            }
            v.push(TreeNode::folder(FolderKind::Indexes, path));
            Ok(v)
        }
        ObjectKind::Folder(FolderKind::Columns) => Ok(columns(s, &path)
            .await?
            .into_iter()
            .map(|c| {
                let mut d = c.data_type.clone();
                if c.primary_key {
                    d.push_str(" · PK");
                }
                if c.auto_increment {
                    d.push_str(" · identity");
                }
                d.push_str(if c.nullable { " · null" } else { " · not null" });
                TreeNode::new(ObjectKind::Column, c.name.clone(), path.clone().with_sub(c.name)).detail(d)
            })
            .collect()),
        ObjectKind::Folder(FolderKind::Indexes) => {
            let mut c = s.conn(Some(&db)).await?;
            let obj = D.qualified(path.schema.as_deref(), path.name()?);
            let rows = text_rows(
                &mut c,
                "SELECT i.name, i.type_desc, CAST(i.is_unique AS int), CAST(i.is_primary_key AS int), \
                 STUFF((SELECT ', ' + c.name FROM sys.index_columns ic JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
                        WHERE ic.object_id = i.object_id AND ic.index_id = i.index_id AND ic.is_included_column = 0 ORDER BY ic.key_ordinal FOR XML PATH('')), 1, 2, '') \
                 FROM sys.indexes i WHERE i.object_id = OBJECT_ID(@P1) AND i.name IS NOT NULL ORDER BY i.is_primary_key DESC, i.name",
                &[&obj.as_str()],
            )
            .await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let name = r[0].clone().unwrap_or_default();
                    let kind_s = if r[3].as_deref() == Some("1") { "primary key".to_string() } else if r[2].as_deref() == Some("1") { "unique".to_string() } else { String::new() };
                    let d = [kind_s, r[1].clone().unwrap_or_default().to_lowercase(), format!("({})", r[4].clone().unwrap_or_default())]
                        .into_iter()
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ");
                    TreeNode::new(ObjectKind::Index, name.clone(), path.clone().with_sub(name)).detail(d)
                })
                .collect())
        }
        ObjectKind::Folder(FolderKind::Constraints) => {
            let mut c = s.conn(Some(&db)).await?;
            let obj = D.qualified(path.schema.as_deref(), path.name()?);
            let rows = text_rows(
                &mut c,
                "SELECT name, type_desc FROM sys.objects WHERE parent_object_id = OBJECT_ID(@P1) AND type IN ('PK','UQ','F','C','D') ORDER BY type, name",
                &[&obj.as_str()],
            )
            .await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let name = r[0].clone().unwrap_or_default();
                    TreeNode::new(ObjectKind::Constraint, name.clone(), path.clone().with_sub(name)).detail(r[1].clone().unwrap_or_default().to_lowercase().replace('_', " "))
                })
                .collect())
        }
        ObjectKind::Folder(FolderKind::Triggers) => {
            let mut c = s.conn(Some(&db)).await?;
            let obj = D.qualified(path.schema.as_deref(), path.name()?);
            let rows = text_rows(&mut c, "SELECT name, CASE WHEN is_disabled = 1 THEN 'disabled' ELSE '' END FROM sys.triggers WHERE parent_id = OBJECT_ID(@P1) ORDER BY name", &[&obj.as_str()]).await?;
            Ok(rows
                .into_iter()
                .map(|r| {
                    let name = r[0].clone().unwrap_or_default();
                    TreeNode::new(ObjectKind::Trigger, name.clone(), path.clone().with_sub(name)).detail(r[1].clone().unwrap_or_default()).leaf()
                })
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

pub async fn columns(s: &MsSession, path: &ObjPath) -> Result<Vec<ColumnInfo>> {
    let obj = D.qualified(path.schema.as_deref().or(Some("dbo")), path.name()?);
    let mut c = s.conn(Some(&s.db_of(path))).await?;
    let rows = text_rows(
        &mut c,
        "SELECT c.name, \
           CASE WHEN t.name IN ('varchar','char','varbinary','binary') THEN t.name + '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length AS varchar) END + ')' \
                WHEN t.name IN ('nvarchar','nchar') THEN t.name + '(' + CASE WHEN c.max_length = -1 THEN 'max' ELSE CAST(c.max_length / 2 AS varchar) END + ')' \
                WHEN t.name IN ('decimal','numeric') THEN t.name + '(' + CAST(c.precision AS varchar) + ',' + CAST(c.scale AS varchar) + ')' \
                WHEN t.name IN ('datetime2','time','datetimeoffset') THEN t.name + '(' + CAST(c.scale AS varchar) + ')' \
                ELSE t.name END, \
           CAST(c.is_nullable AS int), dc.definition, \
           CAST(CASE WHEN EXISTS (SELECT 1 FROM sys.indexes i JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
                 WHERE i.object_id = c.object_id AND i.is_primary_key = 1 AND ic.column_id = c.column_id) THEN 1 ELSE 0 END AS int), \
           CAST(c.is_identity AS int), cc.definition, CAST(ep.value AS nvarchar(4000)) \
         FROM sys.columns c JOIN sys.types t ON t.user_type_id = c.user_type_id \
         LEFT JOIN sys.default_constraints dc ON dc.object_id = c.default_object_id \
         LEFT JOIN sys.computed_columns cc ON cc.object_id = c.object_id AND cc.column_id = c.column_id \
         LEFT JOIN sys.extended_properties ep ON ep.major_id = c.object_id AND ep.minor_id = c.column_id AND ep.name = 'MS_Description' \
         WHERE c.object_id = OBJECT_ID(@P1) ORDER BY c.column_id",
        &[&obj.as_str()],
    )
    .await?;
    if rows.is_empty() {
        return Err(Error::invalid(format!("{} not found", path.display())));
    }
    Ok(rows
        .into_iter()
        .map(|r| ColumnInfo {
            name: r[0].clone().unwrap_or_default(),
            data_type: r[1].clone().unwrap_or_default(),
            nullable: r[2].as_deref() == Some("1"),
            default: r[3].clone(),
            primary_key: r[4].as_deref() == Some("1"),
            auto_increment: r[5].as_deref() == Some("1"),
            comment: r[6].clone().map(|g| format!("computed: {g}")).or_else(|| r[7].clone()),
        })
        .collect())
}

pub async fn ddl(s: &MsSession, kind: ObjectKind, path: &ObjPath) -> Result<String> {
    let db = s.db_of(path);
    match kind {
        ObjectKind::View | ObjectKind::Procedure | ObjectKind::Function | ObjectKind::Trigger => {
            let name = if kind == ObjectKind::Trigger { path.sub.clone().unwrap_or_default() } else { path.name()?.to_string() };
            let obj = D.qualified(path.schema.as_deref().or(Some("dbo")), &name);
            let mut c = s.conn(Some(&db)).await?;
            let rows = text_rows(&mut c, "SELECT OBJECT_DEFINITION(OBJECT_ID(@P1))", &[&obj.as_str()]).await?;
            rows.into_iter().next().and_then(|r| r.into_iter().next().flatten()).map(|d| format!("{}\nGO\n", d.trim())).ok_or_else(|| Error::invalid("Definition not available (encrypted or insufficient permission)"))
        }
        ObjectKind::Table => {
            let cols = columns(s, path).await?;
            let target = D.qualified(path.schema.as_deref().or(Some("dbo")), path.name()?);
            let mut lines: Vec<String> = cols
                .iter()
                .map(|c| {
                    if let Some(expr) = c.comment.as_deref().and_then(|x| x.strip_prefix("computed: ")) {
                        return format!("    {} AS {expr}", D.quote_ident(&c.name));
                    }
                    let mut l = format!("    {} {}", D.quote_ident(&c.name), c.data_type);
                    if c.auto_increment {
                        l.push_str(" IDENTITY(1,1)");
                    }
                    l.push_str(if c.nullable { " NULL" } else { " NOT NULL" });
                    if let Some(d) = &c.default {
                        l.push_str(&format!(" DEFAULT {d}"));
                    }
                    l
                })
                .collect();
            let pk: Vec<String> = cols.iter().filter(|c| c.primary_key).map(|c| D.quote_ident(&c.name)).collect();
            if !pk.is_empty() {
                lines.push(format!("    PRIMARY KEY ({})", pk.join(", ")));
            }
            let mut out = format!("CREATE TABLE {target} (\n{}\n);\n", lines.join(",\n"));
            let mut c = s.conn(Some(&db)).await?;
            let obj = target.clone();
            let idx = text_rows(
                &mut c,
                "SELECT i.name, CAST(i.is_unique AS int), i.type_desc, \
                 STUFF((SELECT ', ' + QUOTENAME(c.name) + CASE WHEN ic.is_descending_key = 1 THEN ' DESC' ELSE '' END FROM sys.index_columns ic \
                        JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
                        WHERE ic.object_id = i.object_id AND ic.index_id = i.index_id AND ic.is_included_column = 0 ORDER BY ic.key_ordinal FOR XML PATH('')), 1, 2, '') \
                 FROM sys.indexes i WHERE i.object_id = OBJECT_ID(@P1) AND i.is_primary_key = 0 AND i.name IS NOT NULL AND i.is_unique_constraint = 0",
                &[&obj.as_str()],
            )
            .await?;
            for r in idx {
                out.push_str(&format!(
                    "CREATE {}{} INDEX {} ON {target} ({});\n",
                    if r[1].as_deref() == Some("1") { "UNIQUE " } else { "" },
                    r[2].clone().unwrap_or_default().replace("NONCLUSTERED", "NONCLUSTERED").split(' ').next().unwrap_or(""),
                    D.quote_ident(r[0].as_deref().unwrap_or_default()),
                    r[3].clone().unwrap_or_default()
                ));
            }
            let fks = text_rows(
                &mut c,
                "SELECT fk.name, \
                 STUFF((SELECT ', ' + QUOTENAME(COL_NAME(fc.parent_object_id, fc.parent_column_id)) FROM sys.foreign_key_columns fc WHERE fc.constraint_object_id = fk.object_id FOR XML PATH('')), 1, 2, ''), \
                 QUOTENAME(OBJECT_SCHEMA_NAME(fk.referenced_object_id)) + '.' + QUOTENAME(OBJECT_NAME(fk.referenced_object_id)), \
                 STUFF((SELECT ', ' + QUOTENAME(COL_NAME(fc.referenced_object_id, fc.referenced_column_id)) FROM sys.foreign_key_columns fc WHERE fc.constraint_object_id = fk.object_id FOR XML PATH('')), 1, 2, '') \
                 FROM sys.foreign_keys fk WHERE fk.parent_object_id = OBJECT_ID(@P1)",
                &[&obj.as_str()],
            )
            .await?;
            for r in fks {
                let g = |i: usize| r[i].clone().unwrap_or_default();
                out.push_str(&format!("ALTER TABLE {target} ADD CONSTRAINT {} FOREIGN KEY ({}) REFERENCES {} ({});\n", D.quote_ident(&g(0)), g(1), g(2), g(3)));
            }
            Ok(out)
        }
        ObjectKind::Index | ObjectKind::Constraint | ObjectKind::Column => Box::pin(ddl(s, ObjectKind::Table, &ObjPath { sub: None, ..path.clone() })).await,
        ObjectKind::Database => Ok(format!("CREATE DATABASE {};\n", D.quote_ident(&db))),
        ObjectKind::Schema => Ok(format!("CREATE SCHEMA {};\n", D.quote_ident(path.schema.as_deref().unwrap_or("dbo")))),
        ObjectKind::Sequence => {
            let mut c = s.conn(Some(&db)).await?;
            let obj = D.qualified(path.schema.as_deref().or(Some("dbo")), path.name()?);
            let r = text_rows(&mut c, "SELECT TYPE_NAME(user_type_id), CAST(start_value AS varchar(40)), CAST(increment AS varchar(40)) FROM sys.sequences WHERE object_id = OBJECT_ID(@P1)", &[&obj.as_str()]).await?;
            let r = r.into_iter().next().ok_or_else(|| Error::invalid("Sequence not found"))?;
            Ok(format!("CREATE SEQUENCE {obj} AS {} START WITH {} INCREMENT BY {};\n", r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default(), r[2].clone().unwrap_or_default()))
        }
        k => Err(Error::unsupported(format!("No DDL for {}", k.label()))),
    }
}

pub async fn table_page(s: &MsSession, q: TableQuery) -> Result<TablePage> {
    let db = s.db_of(&q.path);
    let cols = columns(s, &q.path).await?;
    let pk: Vec<String> = cols.iter().filter(|c| c.primary_key).map(|c| c.name.clone()).collect();
    let target = D.qualified(q.path.schema.as_deref().or(Some("dbo")), q.path.name()?);
    let mut c = s.conn(Some(&db)).await?;
    let otype = text_rows(&mut c, "SELECT RTRIM(type) FROM sys.objects WHERE object_id = OBJECT_ID(@P1)", &[&target.as_str()])
        .await?
        .into_iter()
        .next()
        .and_then(|r| r.into_iter().next().flatten())
        .unwrap_or_default();
    let mut sql = format!("SELECT * FROM {target}");
    if let Some(f) = q.filter.as_deref().filter(|f| !f.trim().is_empty()) {
        single_statement(f)?;
        sql.push_str(&format!(" WHERE {f}"));
    }
    let order: Vec<String> = if q.order_by.is_empty() {
        pk.iter().map(|c| D.quote_ident(c)).collect()
    } else {
        q.order_by.iter().map(|(c, d)| format!("{} {}", D.quote_ident(c), if *d == SortDir::Asc { "ASC" } else { "DESC" })).collect()
    };
    // OFFSET/FETCH requires an ORDER BY.
    sql.push_str(&format!(" ORDER BY {}", if order.is_empty() { "(SELECT NULL)".to_string() } else { order.join(", ") }));
    sql.push_str(&format!(" OFFSET {} ROWS FETCH NEXT {} ROWS ONLY", q.offset, q.limit));
    let mut rows = result(&mut c, &sql, &[]).await?;
    for col in &mut rows.columns {
        if let Some(info) = cols.iter().find(|x| x.name == col.name) {
            *col = ColumnMeta::new(col.name.clone(), info.data_type.clone());
        }
    }
    let read_only_reason = if s.target.read_only {
        Some("Connection is read-only".to_string())
    } else if otype != "U" {
        Some("Views are read-only here".to_string())
    } else if pk.is_empty() {
        Some("Table has no primary key; SQL Server has no stable row id to edit by".to_string())
    } else {
        None
    };
    Ok(TablePage { rows, key_columns: pk, row_ids: None, read_only_reason })
}

pub async fn apply_changes(s: &MsSession, path: &ObjPath, changes: Vec<RowChange>) -> Result<u64> {
    if s.target.read_only {
        return Err(Error::invalid("Connection is read-only"));
    }
    let cols = columns(s, path).await?;
    let target = D.qualified(path.schema.as_deref().or(Some("dbo")), path.name()?);
    let mut c = s.conn(Some(&s.db_of(path))).await?;
    c.simple_query("BEGIN TRANSACTION").await.map_err(ms_err)?.into_results().await.map_err(ms_err)?;
    let mut total = 0;
    let res: Result<()> = async {
        for ch in changes {
            // Values travel as nvarchar parameters; SQL Server converts them to the column type.
            let mut params: Vec<Option<String>> = Vec::new();
            let mut ph = |v: Option<String>, params: &mut Vec<Option<String>>| {
                params.push(v);
                format!("@P{}", params.len())
            };
            let key_where = |key: &RowKey, params: &mut Vec<Option<String>>, ph: &mut dyn FnMut(Option<String>, &mut Vec<Option<String>>) -> String| -> Result<String> {
                match key {
                    RowKey::RowId(_) => Err(Error::unsupported("SQL Server rows are edited by primary key")),
                    RowKey::PrimaryKey(k) => Ok(k
                        .iter()
                        .map(|(c, v)| if v.is_null() { format!("{} IS NULL", D.quote_ident(c)) } else { format!("{} = {}", D.quote_ident(c), ph(v.to_text(), params)) })
                        .collect::<Vec<_>>()
                        .join(" AND ")),
                }
            };
            let sql = match &ch {
                RowChange::Update { key, values } => {
                    let sets: Vec<String> = values.iter().map(|(c, v)| format!("{} = {}", D.quote_ident(c), ph(v.clone(), &mut params))).collect();
                    let w = key_where(key, &mut params, &mut ph)?;
                    format!("UPDATE {target} SET {} WHERE {w}", sets.join(", "))
                }
                RowChange::Insert { values } => {
                    let values: Vec<&(String, Option<String>)> = values.iter().filter(|(c, _)| !cols.iter().any(|x| &x.name == c && x.auto_increment)).collect();
                    if values.is_empty() {
                        format!("INSERT INTO {target} DEFAULT VALUES")
                    } else {
                        let names: Vec<String> = values.iter().map(|(c, _)| D.quote_ident(c)).collect();
                        let vals: Vec<String> = values.iter().map(|(_, v)| ph(v.clone(), &mut params)).collect();
                        format!("INSERT INTO {target} ({}) VALUES ({})", names.join(", "), vals.join(", "))
                    }
                }
                RowChange::Delete { key } => {
                    let w = key_where(key, &mut params, &mut ph)?;
                    format!("DELETE FROM {target} WHERE {w}")
                }
            };
            let refs: Vec<&dyn tiberius::ToSql> = params.iter().map(|p| p as &dyn tiberius::ToSql).collect();
            let n = c.execute(sql, &refs).await.map_err(ms_err)?.total();
            if n == 0 {
                return Err(Error::server("A row was not found (changed or deleted by someone else?). No changes were saved."));
            }
            total += n;
        }
        Ok(())
    }
    .await;
    match res {
        Ok(()) => {
            c.simple_query("COMMIT TRANSACTION").await.map_err(ms_err)?.into_results().await.map_err(ms_err)?;
            Ok(total)
        }
        Err(e) => {
            if let Ok(st) = c.simple_query("IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION").await {
                let _ = st.into_results().await;
            }
            Err(e)
        }
    }
}

pub async fn catalog(s: &MsSession, database: Option<&str>) -> Result<Catalog> {
    let mut c = s.conn(database).await?;
    let rows = text_rows(
        &mut c,
        "SELECT TOP 50000 SCHEMA_NAME(o.schema_id), o.name, RTRIM(o.type), c.name FROM sys.objects o \
         LEFT JOIN sys.columns c ON c.object_id = o.object_id WHERE o.type IN ('U','V') AND o.is_ms_shipped = 0 ORDER BY 1, 2, c.column_id",
        &[],
    )
    .await?;
    let mut cat = Catalog::default();
    for r in rows {
        let (schema, t, ty) = (r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default(), r[2].clone().unwrap_or_default());
        let e = cat.columns.entry(t.clone()).or_default();
        if e.is_empty() {
            if !cat.schemas.contains(&schema) {
                cat.schemas.push(schema.clone());
            }
            cat.objects.push((schema, t, if ty == "V" { ObjectKind::View } else { ObjectKind::Table }));
        }
        if let Some(col) = r[3].clone() {
            e.push(col);
        }
    }
    cat.functions = text_rows(&mut c, "SELECT name FROM sys.objects WHERE type IN ('P','FN','IF','TF') AND is_ms_shipped = 0", &[]).await?.into_iter().filter_map(|r| r.into_iter().next().flatten()).collect();
    Ok(cat)
}

pub async fn drop_object(s: &MsSession, kind: ObjectKind, path: &ObjPath, cascade: bool) -> Result<()> {
    let db = s.db_of(path);
    let schema = path.schema.as_deref().or(Some("dbo"));
    let sql = match kind {
        ObjectKind::Database => {
            if SYSTEM_DBS.contains(&db.as_str()) {
                return Err(Error::invalid("System databases cannot be dropped"));
            }
            let q = D.quote_ident(&db);
            let pre = if cascade { format!("ALTER DATABASE {q} SET SINGLE_USER WITH ROLLBACK IMMEDIATE;\n") } else { String::new() };
            return s.run_on(Some("master"), &format!("{pre}DROP DATABASE {q}")).await.map(|_| ());
        }
        ObjectKind::Schema => format!("DROP SCHEMA {}", D.quote_ident(schema.unwrap_or("dbo"))),
        ObjectKind::Index => format!("DROP INDEX {} ON {}", D.quote_ident(path.sub.as_deref().unwrap_or_default()), D.qualified(schema, path.name()?)),
        ObjectKind::Trigger => format!("DROP TRIGGER {}", D.qualified(schema, path.sub.as_deref().unwrap_or_default())),
        ObjectKind::Constraint => format!("ALTER TABLE {} DROP CONSTRAINT {}", D.qualified(schema, path.name()?), D.quote_ident(path.sub.as_deref().unwrap_or_default())),
        ObjectKind::Column => format!("ALTER TABLE {} DROP COLUMN {}", D.qualified(schema, path.name()?), D.quote_ident(path.sub.as_deref().unwrap_or_default())),
        k => match k.sql_keyword() {
            Some(kw) if !matches!(k, ObjectKind::MaterializedView | ObjectKind::ForeignTable | ObjectKind::Event) => format!("DROP {kw} {}", D.qualified(schema, path.name()?)),
            _ => return Err(Error::unsupported(format!("SQL Server has no {}", k.label()))),
        },
    };
    s.run_on(Some(&db), &sql).await.map(|_| ())
}

pub async fn metrics(s: &MsSession) -> Result<Vec<Metric>> {
    let mut c = s.conn(Some("master")).await?;
    let r = text_rows(
        &mut c,
        "SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(64)), \
         CONVERT(varchar(19), (SELECT sqlserver_start_time FROM sys.dm_os_sys_info), 120), \
         (SELECT COUNT(*) FROM sys.dm_exec_sessions WHERE is_user_process = 1), \
         (SELECT COUNT(*) FROM sys.dm_exec_requests WHERE session_id <> @@SPID AND status IN ('running','runnable','suspended')), \
         (SELECT COUNT(*) FROM sys.dm_exec_requests WHERE blocking_session_id <> 0), \
         (SELECT cntr_value FROM sys.dm_os_performance_counters WHERE counter_name = 'Page life expectancy' AND object_name LIKE '%Buffer Manager%'), \
         (SELECT CAST(100.0 * a.cntr_value / NULLIF(b.cntr_value, 0) AS decimal(6,2)) FROM sys.dm_os_performance_counters a \
            JOIN sys.dm_os_performance_counters b ON b.object_name = a.object_name AND b.counter_name = 'Buffer cache hit ratio base' \
            WHERE a.counter_name = 'Buffer cache hit ratio' AND a.object_name LIKE '%Buffer Manager%'), \
         (SELECT cntr_value FROM sys.dm_os_performance_counters WHERE counter_name = 'Batch Requests/sec'), \
         (SELECT cntr_value FROM sys.dm_os_performance_counters WHERE counter_name = 'Number of Deadlocks/sec' AND instance_name = '_Total'), \
         (SELECT physical_memory_in_use_kb / 1024 FROM sys.dm_os_process_memory), \
         (SELECT COUNT(*) FROM sys.databases), \
         (SELECT COUNT(*) FROM sys.databases WHERE state_desc <> 'ONLINE'), \
         (SELECT cpu_count FROM sys.dm_os_sys_info), \
         CAST(SERVERPROPERTY('Edition') AS nvarchar(128)), \
         (SELECT TOP 1 wait_type FROM sys.dm_os_wait_stats WHERE wait_type NOT LIKE '%SLEEP%' AND wait_type NOT LIKE '%IDLE%' AND wait_type NOT LIKE '%QUEUE%' AND wait_type NOT IN ('BROKER_TASK_STOP','DIRTY_PAGE_POLL','XE_TIMER_EVENT','CHECKPOINT_QUEUE','REQUEST_FOR_DEADLOCK_SEARCH','SQLTRACE_INCREMENTAL_FLUSH_SLEEP','HADR_FILESTREAM_IOMGR_IOCOMPLETION','XE_DISPATCHER_WAIT','LOGMGR_QUEUE','WAITFOR','SOS_WORK_DISPATCHER') ORDER BY wait_time_ms DESC)",
        &[],
    )
    .await?;
    let r = r.into_iter().next().unwrap_or_default();
    let g = |i: usize| r.get(i).cloned().flatten().unwrap_or_else(|| "–".into());
    Ok(vec![
        Metric::new("Server", "Version", g(0)),
        Metric::new("Server", "Edition", g(13)),
        Metric::new("Server", "Started", g(1)),
        Metric::new("Server", "CPUs", g(12)),
        Metric::new("Server", "Memory in use (MB)", g(9)),
        Metric::new("Server", "Databases", g(10)),
        Metric::new("Server", "Databases not online", g(11)),
        Metric::new("Sessions", "User sessions", g(2)),
        Metric::new("Sessions", "Active requests", g(3)),
        Metric::new("Sessions", "Blocked requests", g(4)).hint("Requests waiting on another session's locks"),
        Metric::new("Throughput", "Batch requests (cumulative)", g(7)),
        Metric::new("Memory", "Buffer cache hit ratio (%)", g(6)),
        Metric::new("Memory", "Page life expectancy (s)", g(5)).hint("Seconds a page stays in memory; low values mean memory pressure"),
        Metric::new("Problems", "Deadlocks (cumulative)", g(8)),
        Metric::new("Problems", "Top wait type", g(14)).hint("Largest cumulative wait since start"),
    ])
}

