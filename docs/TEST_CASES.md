# OpenVClient test-case catalogue

This catalogue lists manual and automated test cases. Each case has a stable ID. When a Rust test covers a case (fully or partly), the **Automated** column names it as `crate::test_fn`. "Manual" means nobody has written an automated test yet.

**How to use it**
- Run **all automated tests** with `cargo test --workspace -- --nocapture`. Live tests print `skipping: …` when their server is down. See [CONTRIBUTING.md §5](../CONTRIBUTING.md#5-testing) for the service containers.
- Run the **headless UI walkthrough** with `OPENVCLIENT_CONFIG_DIR=<workspace copy> OPENVCLIENT_SHOTS=<dir> cargo test -p openvclient-app ui_walkthrough -- --ignored --nocapture`.
- Before a release, run the **manual** cases marked **R** (release-blocking) on each platform in the [release matrix](RELEASE_PLAN.md#test-matrix).
- Record results in the release checklist issue in the form `ID: pass/fail/n.a. (OS, version, server)`.

**Common preconditions**
- **P-WS**: a fresh workspace (`OPENVCLIENT_CONFIG_DIR` pointing at an empty dir).
- **P-SVC**: the live test services are running (CONTRIBUTING §5.2).
- **P-DEMO**: `demo/shop.db` and the demo Postgres database (`OPENVCLIENT_DEMO_DB`, default `openv_demo`) exist.

### Automated test index

| Crate | Test function | Kind |
|---|---|---|
| openvclient-core | `sqltext::tests::{keywords_sorted, splits_simple_and_strings, pg_dollar_quotes, sqlite_trigger_body, begin_transaction_is_statement, mysql_delimiter, mysql_procedure_without_delimiter, tsql_go_and_brackets, cursor_lookup, leading}` | unit |
| openvclient-core | `explain::tests::{mysql_tree, pg_json}` | unit |
| openvclient-core | `kv::tests::split` | unit |
| openvclient-app | `export::tests::csv_and_sql` | unit |
| openvclient-app | `views::palette::tests::fuzzy_prefers_contiguous` | unit |
| openvclient-app | `widgets::sqledit::tests::{alias_resolution, completes_columns_after_alias}` | unit |
| openvclient-app | `app::walkthrough::ui_walkthrough` | headless UI, `#[ignore]` |
| openvclient-sqlite | `tests::end_to_end` | file-based, always runs |
| openvclient-postgres | `tests::live_postgres` | live |
| openvclient-mysql | `tests::live_mysql` | live |
| openvclient-sqlserver | `tests::live_sqlserver`, `tests::auth_method_validation` | live / validation |
| openvclient-redis | `tests::live_redis` | live |
| openvclient-mongo | `tests::live_mongo` | live |
| openvclient-http | `clickhouse::tests::live_clickhouse`, `elastic::tests::live_elasticsearch` | live |
| openvclient-http | `rabbitmq::tests::live_rabbitmq` | live (connector in development) |
| openvclient-storage | `s3::tests::live_minio` | live (connector in development) |
| openvclient-docker | `tests::live_docker` | live |
| openvclient-net | `tests::socks5_and_http_proxies`, `tests::ssh_tunnel_password_key_and_pinning` | live |

---

## 1. Connections and workspace (CONN)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| CONN-001 | Create | P-WS | ⌘/Ctrl+N → pick PostgreSQL → fill in host, user and password → Save | The profile appears in the sidebar root. `workspace.json` contains the profile **without** the password. | Manual | R |
| CONN-002 | Driver picker | P-WS | ⌘N, then type "maria" in the picker filter | Only MariaDB matches. Enter selects it. Categories follow `Category::label`. | Manual | |
| CONN-003 | Test connection | P-SVC | In the form, click Test with good credentials, then with a wrong password | Good: shows success with the server product and version. Bad: shows the Auth error and the credentials hint. Nothing is saved. | Manual | R |
| CONN-004 | Edit | CONN-001 | Right-click the profile → Edit connection… → change the port → Save | The change persists after a restart. Leaving the password empty keeps the saved one ("saved" placeholder). | Manual | R |
| CONN-005 | Duplicate | CONN-001 | Context menu → Duplicate | A new profile "… copy" appears with a new id. The secret is copied to the new keychain entry, or you are prompted for it. | Manual | |
| CONN-006 | Delete | CONN-001 | Context menu → Delete… → confirm | The profile is removed and its keychain entries (main, SSH and proxy secrets) are deleted. Its open tabs close. | Manual | R |
| CONN-007 | Folders: create | P-WS | Sidebar → New folder, then New subfolder inside it | Nested folders render. The path is stored as `A/B` in the profile `folder` field. | Manual | |
| CONN-008 | Folders: rename | CONN-007 with profiles inside | Folder menu → Rename folder… → new name | Every profile under the old path (including subfolders) moves to the new path, and its expanded state is kept. | Manual | R |
| CONN-009 | Folders: move profile | CONN-007 | Profile menu → Move to folder… → choose a folder, then the root | The profile moves. The change persists after a restart. | Manual | R |
| CONN-010 | Folders: new connection here | CONN-007 | Folder menu → New connection here | The form opens with the folder pre-filled. The profile is saved in that folder. | Manual | |
| CONN-011 | Environment tag | CONN-001 | Set environment = Production | The sidebar and tab show a production badge in the environment colour. CONN-013 applies. | Manual | |
| CONN-012 | Read-only profile | P-SVC | Tick Read-only, connect, try to edit a row or run DDL | Grid editing is disabled with a reason. Server-side writes are refused with a read-only error and hint. | Manual | R |
| CONN-013 | Production confirm | CONN-011, setting on | Drop a table on the production connection | A dialog asks you to type the connection name. A wrong name keeps the button disabled. | Manual | R |
| CONN-014 | Connect / disconnect | P-SVC | Connect → Disconnect → Connect | The status dot changes from connecting to connected to idle. The "Connected to …" toast appears. `last_connected_at` is updated. | Manual | R |
| CONN-015 | Health ping | Connected | Stop the server container | Within a few pings the connection shows degraded or failed with a reconnect option. No UI freeze. | Manual | |
| CONN-016 | Connect timeout | P-WS | Connect to `10.255.255.1` | It fails after ≤45 s (or the driver timeout) with the "did not answer in time" hint. Cancelling is possible. | Manual | |
| CONN-017 | Workspace corrupt | P-WS | Write invalid JSON to `workspace.json`, start the app | The app starts empty. A toast reports the error and that `workspace.broken-<ts>.json` was kept. | Manual | R |
| CONN-018 | Autosave | P-WS | Make several edits quickly, then kill the app after 1 s | All edits persist (autosave runs about 800 ms after a change). | Manual | |
| CONN-019 | Config dir override | — | Start with `OPENVCLIENT_CONFIG_DIR=/tmp/x` | `workspace.json` and `logs/` are created under `/tmp/x`. The real workspace is untouched. | Manual | |
| CONN-020 | Legacy migration | Old `dev.OpenV.OpenV` workspace exists, new one doesn't | Start the app | The old workspace is imported into the new location. | Manual | |

## 2. Keychain (KEY)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| KEY-001 | Save secret | P-WS | Create a profile with a password and "save secret" on | The OS keychain has an entry with service `dev.openvclient.client` and account `<profile uuid>`. | Manual | R |
| KEY-002 | Don't save secret | P-WS | Untick save secret → connect | You are prompted for the password on connect. It is kept in memory for the session only, and the keychain has no entry. | Manual | R |
| KEY-003 | Network secrets | SSH profile | Save an SSH password, key passphrase or proxy password | Separate entries `<uuid>:<name>` are created. The profile JSON contains none of them. | Manual | R |
| KEY-004 | Keychain denied | macOS | Deny Keychain access when prompted | A toast says "Keychain unavailable: …". The app keeps working and asks for the password. | Manual | |
| KEY-005 | Linux without Secret Service | Linux, no gnome-keyring or KWallet | Save a profile with a password | A clear error toast appears, there is no crash, and the password prompt is offered. | Manual | R (Linux) |
| KEY-006 | Windows Credential Manager | Windows | KEY-001 | The entry is visible in Credential Manager → Windows Credentials. | Manual | R (Win) |
| KEY-007 | Ad-hoc signed bundle | macOS `.app` from the bundle script | Rebuild the bundle and reconnect | The keychain item is still readable (consistent signature). At most one "Always allow" prompt. | Manual | R (mac) |

## 3. SQLite (SQLITE)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| SQLITE-001 | Open / create file | — | New SQLite → pick `demo/shop.db`, or a new path via the save dialog | Connects. A new path creates the database file. | `openvclient-sqlite::tests::end_to_end` | R |
| SQLITE-002 | Browse | SQLITE-001 | Expand the tree | Tables, views, indexes and triggers are listed. DDL is shown for each object. | `end_to_end` | R |
| SQLITE-003 | Query | SQLITE-001 | Run a multi-statement script that includes a trigger body | The statements are split correctly and each result shows in its own tab. | `end_to_end`, `openvclient-core::sqltext::tests::sqlite_trigger_body` | R |
| SQLITE-004 | Edit with PK | SQLITE-001 | Edit a cell, add a row, delete a row → Save | The changes apply in one transaction. The grid refreshes. | `end_to_end` | R |
| SQLITE-005 | Edit without PK | Table without PK | Edit a cell | The row is addressed by `rowid`, and the edit succeeds. | `end_to_end` | |
| SQLITE-006 | Explain | SQLITE-001 | ⌘E on a SELECT | `EXPLAIN QUERY PLAN` is shown as a tree or diagram. Analyze is hidden or gives the explanation "SQLite has no EXPLAIN ANALYZE". | Manual | |
| SQLITE-007 | Backup | SQLITE-001 | Database menu → Back up… → choose a file | Online backup is written. The toast shows the size and time. | `end_to_end` | R |
| SQLITE-008 | Restore | SQLITE-007 | Restore from file… → confirm | The database is replaced by the backup. This is hidden for read-only profiles. | Manual | |
| SQLITE-009 | Read-only file | `chmod 444` file | Connect and try an edit | A read-only error appears with a hint. | Manual | |

## 4. PostgreSQL, Kingbase, GaussDB (PG)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| PG-001 | Connect | P-SVC | Connect with each sslmode: disable, prefer, require | Connects. The header shows `PostgreSQL 16.x`, encoding and user. | `openvclient-postgres::tests::live_postgres` | R |
| PG-002 | Browse | PG-001 | Databases → schemas → tables, views, matviews, sequences, functions (with signature), triggers | Each folder lists its objects. Function labels look like `add(a integer, b integer)`. | `live_postgres` | R |
| PG-003 | DDL | PG-002 | Structure & DDL on a table with an index and a comment | The DDL includes `CREATE TABLE`, the indexes and `COMMENT ON`. | `live_postgres` | R |
| PG-004 | Query types | PG-001 | Select arrays, numeric, date, NULL, json and bytea | Values render correctly. NULLs are distinct from empty strings. | `live_postgres` | R |
| PG-005 | Error stops script | PG-001 | Run all with an error in statement 2 of 3 | Statement 1 succeeds. Statement 2 shows the error with SQLSTATE detail. Statement 3 does not run. | `live_postgres` | R |
| PG-006 | `$$` bodies | PG-001 | Run `CREATE FUNCTION … AS $$ … ; … $$` | It runs as one statement. | `openvclient-core::sqltext::tests::pg_dollar_quotes` | |
| PG-007 | Edit | PG-001 | Edit, insert and delete on a table with a PK, and on a table without one (`ctid`) | The changes apply. Without a PK, the `ctid` addressing works. | `live_postgres` | R |
| PG-008 | Server-side filter/sort/page | Large table | Filter `id > 100`, sort a column, go to the next page, Count | The SQL runs on the server. The count matches the filter. | `live_postgres` | R |
| PG-009 | Cancel | PG-001 | `SELECT pg_sleep(30)` → ⌘. | The query is cancelled within about 1 s with a Cancelled result. The console stays usable. | `live_postgres` | R |
| PG-010 | Explain / Analyze | PG-001 | ⌘E on a SELECT. Analyze a DELETE. | The plan tree and diagram show. ANALYZE of DML runs in a rolled-back transaction, so no rows are deleted. | `live_postgres`, `openvclient-core::explain::tests::pg_json` | R |
| PG-011 | Create/drop database | PG-001 | New database… (owner, encoding, template) → Drop | The database is created and then dropped. Drop asks for confirmation. | `live_postgres` | |
| PG-012 | Allow connections toggle | PG-011 | Toggle allow connections | `ALLOW_CONNECTIONS` changes. Labelled as a toggle, not as "offline". | Manual | |
| PG-013 | Monitor | PG-001 | Open Monitor | Metrics with history, sessions and locks show. Kill query and terminate session work on a sleeping session. | `live_postgres` (activity) | R |
| PG-014 | Users & roles | PG-001 | Create a role, set its password, grant SELECT on a table, revoke, drop | Each step succeeds and the privilege list updates. | `live_postgres` | R |
| PG-015 | Backup/restore | `pg_dump`/`pg_restore` on PATH | Back up a database (custom format) and restore it into a new database | The round trip preserves the data. When the tools are missing, the actions are hidden. A version mismatch gives a clear error. | `live_postgres` (when `pg_dump` found) | R |
| PG-016 | Export | Result set | Export to CSV, TSV, JSON, SQL INSERT and Markdown | Each file is valid, NULL is handled, and identifiers are quoted for Postgres. | `openvclient-app::export::tests::csv_and_sql` | R |
| PG-017 | Import CSV | Table | Import CSV… with a header that matches the columns | A confirmation dialog shows the row count. Rows are inserted in one transaction. Empty cells become NULL. | Manual | R |
| PG-018 | Kingbase flavour | Kingbase server | Connect with the Kingbase driver (port 54321) | Connects. Catalog-dependent features are labelled PostgreSQL-compatible. | Manual (no public image) | |
| PG-019 | GaussDB / openGauss | `opengauss/opengauss` container | Connect with MD5 and with SCRAM auth | Connects, browses and queries. | Manual | |

## 5. MySQL and MariaDB (MY)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| MY-001 | Connect | P-SVC | Connect as root, with TLS off and with TLS on | The header shows the product and version. | `openvclient-mysql::tests::live_mysql` | R |
| MY-002 | MariaDB mode | `mariadb:11` | Connect with the MariaDB driver | The product shows MariaDB. `mariadb-dump` is preferred for backup. | Manual | R |
| MY-003 | Browse | MY-001 | Tables, views, procedures, functions, triggers, events | Each folder lists its objects. DDL is shown for each. | `live_mysql` | R |
| MY-004 | DELIMITER scripts | MY-001 | Run a script with `DELIMITER //` … `//`, and a procedure without DELIMITER | Both are split correctly. | `live_mysql`, `openvclient-core::sqltext::tests::{mysql_delimiter, mysql_procedure_without_delimiter}` | R |
| MY-005 | Edit with and without PK | MY-001 | Edit `users`. Open `nokey`. | Edits on `users` apply. `nokey` is read-only with a reason shown. | `live_mysql` | R |
| MY-006 | Cancel | MY-001 | `SELECT SLEEP(30)` → ⌘. | Cancelled (KILL QUERY). The console is still usable. | `live_mysql` | R |
| MY-007 | Explain / Analyze | MySQL 8.0.18+ | ⌘E. Analyze. | The tree plan and diagram show. ANALYZE of DML is refused or shows a warning. | `live_mysql`, `openvclient-core::explain::tests::mysql_tree` | |
| MY-008 | Users & grants | MY-001 | Create `'u'@'localhost'`, grant, revoke, change password, drop | Each step succeeds. The `user@host` pair is respected. | `live_mysql` | R |
| MY-009 | Backup/restore | `mysqldump` + `mysql` on PATH | Back up a database and restore it into an empty database | The round trip succeeds. When the tools are missing, the actions are hidden. | `live_mysql` (when tools found) | R |
| MY-010 | Monitor | MY-001 | Open Monitor | The process list and metrics show. Kill works. | `live_mysql` (activity) | |
| MY-011 | Access denied hint | P-SVC | Wrong password | The Auth error shows the hint about `'user'@'host'`. | Manual | |

## 6. SQL Server (MS)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| MS-001 | Connect (sql auth) | mssql container on 51433 | auth=`sql`, `sa` and its password | Connects. The product shows SQL Server 2022. | `openvclient-sqlserver::tests::live_sqlserver` | R |
| MS-002 | SSMS tree | MS-001 | Expand the top level | The top level shows Databases, Security and Activity Monitor. System databases sit in their own folder. Each database has Tables, Views, Programmability and Schemas. | `live_sqlserver` | R |
| MS-003 | T-SQL batches | MS-001 | Script with `GO` separators and `[bracketed]` names | It is split on `GO` lines only. | `live_sqlserver`, `openvclient-core::sqltext::tests::tsql_go_and_brackets` | R |
| MS-004 | Edit | MS-001 | Edit on a table with an IDENTITY PK. Open a heap. | Edits apply. The heap is read-only with a reason. | `live_sqlserver` | R |
| MS-005 | Cancel | MS-001 | `WAITFOR DELAY '00:00:30'` → ⌘. | Cancelled with an attention packet. | `live_sqlserver` | R |
| MS-006 | Estimated plan | MS-001 | ⌘E on a SELECT | `SHOWPLAN_ALL` is shown as the graphical plan diagram (see SQL-012). | `live_sqlserver` | R |
| MS-007 | Actual plan | MS-001 | Analyze on a DELETE | `STATISTICS PROFILE` runs. DML is rolled back. | `live_sqlserver` | |
| MS-008 | Offline/online | MS-001 | Database → Take offline → Bring online | The state changes. The offline database's children aren't expanded. | `live_sqlserver` | |
| MS-009 | Server-side backup/restore | MS-001 | Back up to `/var/opt/mssql/data/x.bak`, then restore | The dialog says the path is on the **server**. The round trip succeeds. Hidden for Azure SQL. | `live_sqlserver` | R |
| MS-010 | Logins & users | MS-001 | Create a login, grant, drop | Each step succeeds. | `live_sqlserver` | |
| MS-011 | auth=`windows` | **Windows build**, domain or local account | `DOMAIN\user` and password | Connects with NTLM. On macOS and Linux builds, a clear "Windows build" error appears. | Manual | R (Win) |
| MS-012 | auth=`integrated` | **Windows build**, SQL Server trusting the Windows login | No user or password | Connects as the current Windows user (SSPI). On other OSes the error mentions the Windows build. | `auth_method_validation` (non-Windows error path) | R (Win) |
| MS-013 | auth=`entra_password` | Azure SQL, an Entra user without MFA | user@tenant, password, tenant | Connects. A wrong password gives an Auth error with the Entra message in the detail. | Manual | R |
| MS-014 | auth=`entra_service_principal` | App registration with database access | Client ID, secret, tenant | Connects. A missing client ID gives the error "Client (application) ID …". | `auth_method_validation` (validation) | R |
| MS-015 | auth=`entra_managed_identity` | Azure VM or App Service with MI | Optional client ID for a user-assigned identity | Connects through IMDS or `IDENTITY_ENDPOINT`. Off Azure there is a clear error. | Manual | |
| MS-016 | auth=`entra_interactive` | Azure SQL | Connect | A toast or notice shows the device code and URL. The connection completes after you sign in. Cancelling works. | Manual | R |
| MS-017 | auth=`entra_default` | Each of: `AZURE_*` env vars, Azure host, `az login` | Connect | It uses the first available credential. With none, the error lists all three options. | Manual | |
| MS-018 | auth=`entra_token` | A token from `az account get-access-token --resource https://database.windows.net` | Paste it as the secret | Connects. An empty secret gives an error. | `auth_method_validation` (empty token) | |
| MS-019 | Named instance | SQL Server Browser on UDP 1434, instance `SQLEXPRESS` | Host + instance, empty port | The port is resolved through the Browser. If the Browser is unreachable, the error says so. | Manual | R (Win) |
| MS-020 | Encryption modes | MS-001 | encrypt = on, off, required (self-signed cert) | on: connects (trusts the cert). off: login-only encryption. required: fails with the TLS hint. | Manual | R |

## 7. ClickHouse (CH)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| CH-001 | Connect | ClickHouse on 58123 | HTTP connect | The product shows ClickHouse. | `openvclient-http::clickhouse::tests::live_clickhouse` | R |
| CH-002 | Browse + DDL | CH-001 | Expand database → tables. Open DDL. | The DDL shows the `MergeTree` engine. | `live_clickhouse` | R |
| CH-003 | Query + page | CH-001 | `SELECT` from a 1000-row table. Page through the table browser. | Rows and types render. Paging works. | `live_clickhouse` | R |
| CH-004 | Explain | CH-001 | ⌘E. Analyze. | `EXPLAIN indexes = 1` is shown. Analyze is refused with the message pointing to `EXPLAIN PIPELINE` or `query_log`. | Manual | |
| CH-005 | Create/drop database | CH-001 | New database → drop | Created, then dropped. | `live_clickhouse` | |

## 8. Redis (RD)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| RD-001 | Connect | Redis on 56379 | Connect without auth, and with an ACL user | The product shows Redis or Valkey. The database list shows key counts. | `openvclient-redis::tests::live_redis` | R |
| RD-002 | Scan + filters | RD-001 | Pattern `user:*`, then a type filter | Results page in with SCAN, never KEYS. | `live_redis` | R |
| RD-003 | Create keys | RD-001 | Create string, hash, list, set, zset and stream keys, with and without TTL | Each key is created. Creating an existing key fails. | `live_redis` | R |
| RD-004 | Edit values | RD-003 | Hash set/delete field, list push/set/remove, zset score, string edit | Each edit persists. | `live_redis` | R |
| RD-005 | TTL / rename / delete | RD-003 | Set TTL, persist, rename, bulk delete | Each step succeeds. | `live_redis` | |
| RD-006 | Console | RD-001 | `RPUSH l a b "c d"`, history ↑/↓ | Quoted arguments are parsed correctly and the history works. | `live_redis`, `openvclient-core::kv::tests::split` | R |
| RD-007 | Info / clients / slowlog | RD-001 | Open each panel. Kill a client. | Data shows. Kill succeeds. | `live_redis` | |
| RD-008 | TLS | Redis with TLS | TLS on | Connects, or shows the TLS hint on certificate failure. | Manual | |

## 9. MongoDB (MG)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| MG-001 | Connect | mongo on 57017, root/ovcpass | Connect | The product shows MongoDB 7. | `openvclient-mongo::tests::live_mongo` | R |
| MG-002 | Databases/collections | MG-001 | New database (with first collection), new collection, rename, drop | Each step succeeds and the tree refreshes. | `live_mongo` | R |
| MG-003 | Documents | MG-002 | Insert a document. Filter `{"age": {"$gt": 30}}`. Page through. Update many. Delete many. | The results are correct. Delete many asks for confirmation. | `live_mongo` | R |
| MG-004 | Indexes | MG-002 | Create an index, then drop it | Each step succeeds. | `live_mongo` | |
| MG-005 | Import/export JSON | MG-002 | Export a collection to a file, then import it into a new collection | The round trip preserves the document count. | Manual | R |
| MG-006 | Console | MG-001 | `{"count": "users"}` | The JSON result is shown. | Manual | |

## 10. Elasticsearch (ES)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| ES-001 | Connect | ES on 59200, auth none | Connect | The product shows Elasticsearch 8. | `openvclient-http::elastic::tests::live_elasticsearch` | R |
| ES-002 | Indices | ES-001 | New index with mappings JSON. Close, open, refresh, delete. | Each step succeeds. | `live_elasticsearch` | R |
| ES-003 | Documents | ES-002 | Index a document. Search with a query string. Delete by ID. Delete by query. | The results are correct. | `live_elasticsearch` | R |
| ES-004 | REST console | ES-001 | `GET /_cat/nodes?v` | Text or JSON output is shown. | Manual | |
| ES-005 | Auth modes | Secured cluster | Basic auth and API key | Connects. A wrong key gives an Auth error. | Manual | |

## 11. Docker (DK)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| DK-001 | Connect | Docker daemon running | Connect (default socket or named pipe) | The product shows Docker Engine. System info is listed. | `openvclient-docker::tests::live_docker` | R |
| DK-002 | Container lifecycle | `openvclient-test-redis` | Stop, start, restart, pause, unpause, kill, remove (force) | Each state changes as expected. Remove asks for confirmation. | `live_docker` | R |
| DK-003 | Logs / inspect / stats | DK-002 | Open each | Logs tail with timestamps. Inspect shows JSON. Stats update live. | `live_docker` | R |
| DK-004 | Exec | DK-002 | `redis-cli ping` | The output is `PONG` with exit code 0. | `live_docker` | |
| DK-005 | Images / volumes / networks | DK-001 | Pull `alpine:3`, remove it. List volumes and networks. | Each step succeeds. | Manual | |
| DK-006 | Daemon not running | Daemon stopped | Connect | A connection error appears with a hint. | Manual | |

## 12. RabbitMQ and S3 (in development, not released)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| RMQ-001 | Connect | `rabbitmq:3-management` on 55673, ovc/ovcpass | Connect | The product shows RabbitMQ. A port without the management plugin gives the clear "enable rabbitmq_management" error. | `openvclient-http::rabbitmq::tests::live_rabbitmq` | |
| S3-001 | Connect + buckets | MinIO on 59000, ovcadmin/ovcpass123, path-style | Create a bucket, upload, list, delete prefix, delete bucket | Each step succeeds. | `openvclient-storage::s3::tests::live_minio` | |

Update ROADMAP.md before these become release-blocking.

## 13. Network paths (NET)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| NET-001 | SSH password | SSH server on 52222 (`tunnel`/`tunnelpw`) that can reach the target | Network tab → SSH tunnel, password → connect to Postgres or Redis behind it | Connects. The tab label shows "Network · SSH tunnel". | `openvclient-net::tests::ssh_tunnel_password_key_and_pinning` | R |
| NET-002 | SSH private key | Key authorised on the server | Key path (with `~` expansion), with and without a passphrase | Connects. A wrong passphrase gives an Auth error. | `ssh_tunnel_password_key_and_pinning` (with `OPENVCLIENT_TEST_SSH_KEY`) | R |
| NET-003 | SSH agent | `ssh-agent` with the key loaded (`SSH_AUTH_SOCK`) | Auth = agent | Connects. Without an agent there is a clear error. | Manual (implemented, untested) | R |
| NET-004 | Host key TOFU | NET-001 on a fresh profile | First connect | The SHA-256 fingerprint is saved in the profile params (`SHA256:…`). | `ssh_tunnel_password_key_and_pinning` | R |
| NET-005 | Host key changed | NET-004, then the server key is regenerated | Reconnect | The connection is **refused**. The error mentions the host key and the hint advises checking with the administrator. | `ssh_tunnel_password_key_and_pinning` | R |
| NET-006 | SSH wrong password | NET-001 | Bad password | Auth error kind, with a hint that mentions the SSH login. | `ssh_tunnel_password_key_and_pinning` | |
| NET-007 | SOCKS5 with auth | SOCKS5 on 51080 (`puser`/`ppass`) | Connect through the proxy. Then use a wrong password. | Connects. A bad password fails. Several concurrent connections share one forwarder. | `openvclient-net::tests::socks5_and_http_proxies` | R |
| NET-008 | HTTP CONNECT | HTTP proxy on 53128 | Connect through the proxy. Then use an unresolvable target. | Connects. An unreachable target gives an error. | `socks5_and_http_proxies` | R |
| NET-009 | HTTP proxy auth | Proxy that requires Basic auth | User and password | Connects. A wrong password gives a 407 error. | Manual | |
| NET-010 | Default port through tunnel | NET-001 | Leave the port empty | The driver's default port is used (`conns::default_port`). | Manual | |
| NET-011 | Non-networked drivers | SQLite, Docker | Open the form | No Network tab (no `host` field). | Manual | |

## 14. SQL editor (SQL)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| SQL-001 | New tab | Connected SQL | ⌘/Ctrl+T | A new editor opens for the focused connection. With none, the toast says "Connect to a database first". | `ui_walkthrough` | R |
| SQL-002 | Run statement at cursor | SQL-001 | Put the cursor in the 2nd of 3 statements → ⌘Enter | Only that statement runs. | `openvclient-core::sqltext::tests::cursor_lookup` | R |
| SQL-003 | Run selection | SQL-001 | Select some text → ⌘Enter | Only the selection runs. The button label reads "Run selection". | Manual | R |
| SQL-004 | Run all | SQL-001 | ⌘⇧Enter or F5 | All statements run in order and stop at the first error. Each result has its own tab with the elapsed time. | `ui_walkthrough`, `sqltext::tests::splits_simple_and_strings` | R |
| SQL-005 | Cancel | Long query | ⌘. or the Cancel button | A spinner and timer show while running. The query is cancelled and the console stays usable. | `live_postgres`, `live_mysql`, `live_sqlserver` (cancel) | R |
| SQL-006 | Session state | SQL-001 | `BEGIN; INSERT …;` → then `ROLLBACK;` in a later run | The transaction persists across runs in the same tab (dedicated console). | Manual | R |
| SQL-007 | Max rows | Setting = 100 | `SELECT` 10k rows | 100 rows are kept and the rest are counted. A note shows the truncation. | `live_postgres` (truncation) | |
| SQL-008 | Autocomplete | SQL-001 | Type `SELECT u.` after `FROM users u` | Column suggestions for `users` appear. Enter or Tab accepts. ↑/↓ navigate. | `openvclient-app::widgets::sqledit::tests::{alias_resolution, completes_columns_after_alias}` | R |
| SQL-009 | Syntax highlight | SQL-001 | Keywords, strings, comments, numbers | Colours follow the theme syntax palette (4.5:1 contrast) in both themes. | Manual | |
| SQL-010 | Explain | Engine with explain | ⌘E | The plan is shown. The button is hidden when `capabilities.explain` is false. | `ui_walkthrough`; per-engine live tests | R |
| SQL-011 | Explain analyze | Engine with explain_analyze | Analyze button | The actual plan is shown. DML is rolled back where the engine supports that. | per-engine live tests | R |
| SQL-012 | Graphical plan diagram | SQL-010 | View the plan diagram | The root is on the left with inputs flowing in from the right. Each box shows its share of the cost, and arrow thickness follows the row count. The diagram scrolls and zooms without overlap. | `ui_walkthrough` (screenshot) | R |
| SQL-013 | Save script | SQL-001 | ⌘S | A save dialog writes a `.sql` file. | Manual | |
| SQL-014 | Shortcut scope | Two tabs | Press ⌘Enter while the pointer or focus is in the other pane | Only the focused editor reacts. | Manual | |
| SQL-015 | BEGIN TRANSACTION parsing | SQL-001 | `BEGIN TRANSACTION; …` | It is treated as a statement, not as a block start. | `sqltext::tests::begin_transaction_is_statement` | |

## 15. Data grid and table browser (GRID)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| GRID-001 | Open data | Table | Open data | The first page appears (page size from Settings). The header shows types and PK markers. | `ui_walkthrough` | R |
| GRID-002 | Filter | GRID-001 | Enter a WHERE expression → Apply | Filtered on the server. An invalid expression shows the server error, and the grid is kept. | Manual | R |
| GRID-003 | Sort | GRID-001 | Click a header, then click it again | ASC, then DESC, done on the server. | Manual | R |
| GRID-004 | Paging + count | Large table | Next, previous, Count | Offsets are correct. The count respects the filter. | Manual | |
| GRID-005 | Edit cell / NULL | Editable table | Edit a cell, set NULL, add a row, delete a row → Save or Discard | Pending changes are highlighted. Save applies them all in one transaction. Discard reverts. | Manual | R |
| GRID-006 | Read-only reason | Table without a key | Try to edit | Disabled, with the `read_only_reason` shown. | Manual | R |
| GRID-007 | Copy | GRID-001 | Select cells → copy | The clipboard holds TSV. | Manual | |
| GRID-008 | Export selection | GRID-001 | Export selected rows/columns → Markdown | Only the selection is exported. | `export::tests::csv_and_sql` | |
| GRID-009 | Large values | bytea/blob/long text | View | The value is truncated in the cell with a full-value viewer. The UI stays at 60 fps. | Manual | |
| GRID-010 | Scroll speed | Settings | Change scroll speed | The wheel and trackpad scroll scale accordingly. | Manual | |

## 16. Command palette (PAL)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| PAL-001 | Open | — | ⌘/Ctrl+K, and ⌘/Ctrl+Shift+P | The palette opens with focus in the input. Esc closes it. | `ui_walkthrough` | R |
| PAL-002 | Fuzzy ranking | Several profiles and objects | Type "usr" | Contiguous matches rank higher. ↑/↓ and Enter work. | `openvclient-app::views::palette::tests::fuzzy_prefers_contiguous` | R |
| PAL-003 | Actions | — | Choose New connection, Home, Monitor <conn>, an open tab | Each runs its command. A disconnected profile entry connects. | Manual | |
| PAL-004 | Objects | Connected and expanded | Pick a table | It opens the table data or object view. | Manual | |

## 17. Toasts and error hints (ERR)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| ERR-001 | Refused | — | Connect to a closed port | The hint is "Nothing is listening on that host and port…". | Manual (`Error::hint`) | R |
| ERR-002 | DNS | — | Host `no-such-host.invalid` | The hint is "The host name could not be resolved…". | Manual | |
| ERR-003 | TLS | Self-signed server, verify on | Connect | The TLS hint suggests the SSL/TLS option. | Manual | |
| ERR-004 | Auth | Wrong password | Connect | The credentials hint appears. | Manual | R |
| ERR-005 | Missing database | Wrong database name | Connect | The hint about the database name appears. | Manual | |
| ERR-006 | Copy message | Any error toast | Click copy | The clipboard holds the message and detail. | Manual | |
| ERR-007 | Diagnostics in log | Any error | Check `openvclient.log` | The line holds the kind, message, detail, hint and OS, and **no secrets**. | Manual | R |
| ERR-008 | Unsupported | e.g. SQLite analyze | Trigger it | A clear "unsupported" message that explains the alternative. | Manual | |

## 18. Themes, settings and accessibility (UI)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| UI-001 | Theme cycle | — | Top-bar theme button: system → dark → light → system | The theme applies immediately and persists. | `ui_walkthrough` (light and dark shots) | R |
| UI-002 | System theme | OS in light, then dark | Theme = System | Follows the OS. | Manual | |
| UI-003 | Contrast | Both themes | Inspect text, muted text, accent, destructive | ≥4.5:1 on bg and card, as in MASTER.md. | Manual | R |
| UI-004 | Settings persist | — | Change the editor font, max rows, page size, production confirm and scroll speed. Restart. | The values persist and take effect. | Manual | R |
| UI-005 | Settings paths | — | Settings dialog | Shows the workspace file path and the logs path. Open logs folder opens Finder, Explorer or xdg-open. | Manual | R |
| UI-006 | No emoji / raw hex | Code review | `grep` views for `Color32::from_rgb` and emoji | None. Icons come from `egui_material_icons`. | Manual (review) | |
| UI-007 | Keyboard | — | ⌘N, ⌘T, ⌘W, ⌘⇧H, Esc in dialogs | Each works. On Windows and Linux, Ctrl replaces ⌘. | Manual | R |
| UI-008 | HiDPI | Retina, 150% Windows scaling | Inspect | Text is crisp and the layout isn't clipped. Minimum window is 900×560. | Manual | R |
| UI-009 | Screen reader | AccessKit (VoiceOver, Narrator, Orca) | Navigate the sidebar and buttons | Controls have labels. | Manual | |
| UI-010 | Home | Several profiles | Open Home (⌘⇧H) | Connection overview with health, quick actions and shortcuts. | `ui_walkthrough` | |

## 19. Logging (LOG)

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| LOG-001 | File created | P-WS | Start the app | `<config>/logs/openvclient.log` contains the line `OpenVClient <ver> starting on <os> <arch>`. | Manual | R |
| LOG-002 | Rotation | Log >5 MB | Restart | The old log is renamed to `openvclient.1.log` and a new file is started. | Manual | |
| LOG-003 | Filter | — | `OPENVCLIENT_LOG=debug` | Debug lines appear. The default is `warn,openvclient=info`. | Manual | |
| LOG-004 | No secrets | — | Connect with a password, SSH, a proxy and an Entra token. Grep the log for each secret. | No secret appears. | Manual | R |

## 20. Packaging smoke tests (PKG)

Run these on each release artefact. See [RELEASE_PLAN.md](RELEASE_PLAN.md).

| ID | Area | Preconditions | Steps | Expected result | Automated | R |
|---|---|---|---|---|---|---|
| PKG-001 | macOS .app launch | Downloaded universal `.dmg` | Mount, drag to Applications, open | No Gatekeeper block when notarised. With an ad-hoc build, right-click → Open works. The window title is OpenVClient. | Manual | R |
| PKG-002 | macOS universal | PKG-001 | `lipo -archs OpenVClient.app/Contents/MacOS/OpenVClient` | `x86_64 arm64`. It runs under Rosetta and natively. | CI (release.yml) | R |
| PKG-003 | macOS signature | PKG-001 | `codesign --verify --deep --strict -v` and `spctl -a -vv` | Valid. "Notarized Developer ID" when signed. | CI when secrets are set | R |
| PKG-004 | Windows zip/MSI | Clean Windows 10/11 VM | Unzip and run, or install the MSI | Starts with no console window. SmartScreen shows the publisher when signed. | Manual | R |
| PKG-005 | Windows ARM64 | Windows on ARM | Run the aarch64 build | Starts natively. | Manual | |
| PKG-006 | Linux tar.gz | Ubuntu 22.04/24.04, Fedora 40 | Extract and run `./openvclient` | Starts under X11 and Wayland. Missing Secret Service gives a clear error (KEY-005). | Manual | R |
| PKG-007 | Linux .deb | Ubuntu | `sudo apt install ./openvclient_*.deb` | The desktop entry and icon appear. Dependencies resolve. | Manual | R |
| PKG-008 | Checksums | Any artefact | `sha256sum -c SHA256SUMS` | Every file reports OK. The GPG signature verifies when published. | CI | R |
| PKG-009 | GL fallback | VM without GPU (llvmpipe) | Launch | It renders (software GL), maybe slowly. No crash. | Manual | |
| PKG-010 | Version | Any | Read the first log line or the About info | Matches the tag `vX.Y.Z`. | Manual | R |
| PKG-011 | Feature builds | — | `cargo build -p openvclient-app --no-default-features --features sqlite` | Builds. Only SQLite is offered in the picker. | CI (could be added) | |
