# Contributing to OpenVClient

Thanks for helping. Bug reports, connectors, UI work, tests and docs are all welcome. You don't need a CLA or a DCO sign-off. Open a pull request and we'll review it.

By participating you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md). Please report security issues privately as described in [SECURITY.md](SECURITY.md), not in public issues.

---

## 1. Development setup

1. Install Rust **1.95 or newer** with [rustup](https://rustup.rs). The workspace uses edition 2024. Also install the components: `rustup component add rustfmt clippy`.
2. Install the platform build dependencies listed in the [README](README.md#install). On Linux you need the X11/Wayland headers and `libssl-dev`.
3. Build and run:
   ```sh
   cargo run                         # debug build of the app (default member)
   cargo build --workspace           # everything
   cargo test --workspace            # unit tests + live tests (skip without servers)
   ```
   Dependencies are built with `opt-level = 2` even in debug builds (see `[profile.dev.package."*"]`), so the dev build stays responsive.
4. Optional: start the live test services ([section 5](#5-testing)) so the connector tests run instead of skipping.

Useful environment variables:

| Variable | Effect |
|---|---|
| `OPENVCLIENT_CONFIG_DIR=/tmp/ovc` | Use a throwaway workspace and keep your real profiles safe. |
| `OPENVCLIENT_LOG=debug` | More logging (`tracing` `EnvFilter` syntax). |

---

## 2. Workspace layout

```
crates/
  openvclient-core/       contracts every connector implements + shared types
    driver.rs             Driver, DriverInfo, FieldSpec, ServiceSession, SessionHandle
    sql.rs                SqlSession, SqlConsole, SqlCapabilities, ObjPath, TreeNode…
    kv.rs                 KeyValueSession (Redis-like stores)
    containers.rs         ContainerSession (Docker-like hosts)
    resource.rs           ResourceSession (generic tree + content + actions: Mongo, Elasticsearch)
    sqltext.rs            Dialect, statement splitter, keywords
    explain.rs            ExplainResult / plan tree parsing
    error.rs              Error, ErrorKind, user-facing hints
    profile.rs            ConnectionProfile, ConnectParams, Environment
    registry.rs           DriverRegistry
  openvclient-sqlite/     rusqlite (bundled)
  openvclient-postgres/   tokio-postgres; also registers Kingbase and GaussDB flavours
  openvclient-mysql/      mysql_async; MySQL and MariaDB
  openvclient-sqlserver/  tiberius; SQL logins, Windows auth, Microsoft Entra ID (aad.rs)
  openvclient-redis/      redis
  openvclient-mongo/      mongodb
  openvclient-http/       ClickHouse (HTTP interface) + Elasticsearch (REST); RabbitMQ (in development)
  openvclient-storage/    S3-compatible object storage, FTP/SFTP (in development)
  openvclient-docker/     bollard
  openvclient-net/        SSH tunnel, SOCKS5, HTTP CONNECT (used by the app, not by drivers)
  openvclient-app/        egui UI, binary `openvclient`
    main.rs               driver registration, logging, eframe start-up
    store.rs              workspace.json + OS keychain
    conns.rs              connection lifecycle, tunnels, health pings
    theme.rs              design tokens, icons, buttons
    views/                sidebar, home, SQL editor, table, object, monitor, users, redis, docker, resource, palette, dialogs
    widgets/              grid, SQL editor widget, plan graph, toasts, cards
design-system/openvclient/MASTER.md   the UI design system (read before UI work)
scripts/                  packaging scripts (bundle-macos.sh, bundle-linux.sh, bundle-windows.ps1)
docs/                     TEST_CASES.md, RELEASE_PLAN.md
```

The rule that holds this together is that **the UI only talks to the traits in `openvclient-core`**. A connector describes its own connection form (`FieldSpec`) and its abilities (`SqlCapabilities`, `ActionSpec`). The UI renders what the connector reports, so adding a connector never means editing the form code.

---

## 3. Adding a connector

### 3.1 Pick a family

`Driver::connect` returns a `SessionHandle`. Its variant decides which UI the user gets:

| Variant | Trait | UI you get for free | Examples |
|---|---|---|---|
| `SessionHandle::Sql` | `SqlSession` (+ `SqlConsole`) | object tree, SQL editor, table grid with editing, DDL, export/import, monitor, users, backup | Postgres, MySQL, SQL Server, SQLite, ClickHouse |
| `SessionHandle::KeyValue` | `KeyValueSession` | key browser, value editors, TTL, console, server info | Redis |
| `SessionHandle::Containers` | `ContainerSession` | containers, logs, stats, exec, images, volumes, networks | Docker |
| `SessionHandle::Resource` | `ResourceSession` | generic tree, table/JSON/text viewer, filter box, per-node actions with forms, optional console | MongoDB, Elasticsearch |

For a new kind of service that isn't SQL, start with `ResourceSession`. It is the most flexible family and needs no new UI code.

All four family traits have `ServiceSession` as a supertrait:

```rust
#[async_trait]
pub trait ServiceSession: Send + Sync {
    fn server_info(&self) -> ServerInfo;
    async fn ping(&self) -> Result<Duration>;   // used by the health indicator
    async fn close(&self);
}
```

### 3.2 Create the crate

```sh
cargo new --lib crates/openvclient-foo
```

`crates/openvclient-foo/Cargo.toml`:
```toml
[package]
name = "openvclient-foo"
description = "Foo connector for OpenVClient"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
openvclient-core.workspace = true
async-trait.workspace = true
tokio.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

### 3.3 Minimal skeleton (ResourceSession)

These signatures match `openvclient-core` as it is today. Check `crates/openvclient-core/src/*.rs` if the compiler disagrees.

```rust
//! Foo connector.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use openvclient_core::resource::{ActionOutcome, ActionSpec, Args, Content, Node, NodeIcon, OpenOptions, ResourceSession};
use openvclient_core::{
    Category, ConnectParams, Driver, DriverInfo, Error, FieldKind, FieldSpec, Result, ServerInfo, ServiceSession, SessionHandle,
};

pub struct FooDriver {
    info: DriverInfo,
}

impl Default for FooDriver {
    fn default() -> Self {
        Self {
            info: DriverInfo {
                id: "foo",                      // stable: persisted in profiles
                name: "Foo",
                category: Category::Document,
                description: "Foo 3.x over its native protocol",
                badge: "FO",
                fields: vec![
                    // A `host` field makes the Network tab (SSH / SOCKS5 / HTTP proxy) appear.
                    FieldSpec::text("host", "Host").default("localhost").required(),
                    FieldSpec::text("port", "Port").kind(FieldKind::Number).default("7000"),
                    FieldSpec::text("user", "User"),
                    // Secret fields go to the OS keychain and arrive as `params.secret`.
                    FieldSpec::text("password", "Password").kind(FieldKind::Secret),
                    FieldSpec::text("tls", "TLS").kind(FieldKind::Bool).default("false").advanced(),
                ],
            },
        }
    }
}

#[async_trait]
impl Driver for FooDriver {
    fn info(&self) -> &DriverInfo {
        &self.info
    }

    async fn connect(&self, p: ConnectParams) -> Result<SessionHandle> {
        // If a tunnel is active, the app has already rewritten host/port to its local end.
        let host = p.get_or("host", "localhost").to_string();
        let port = p.port(7000)?;
        // ... open the client; map library errors to Error::connection / auth / server ...
        let _ = (&host, port, p.secret.as_deref(), p.flag("tls"), p.read_only);
        Ok(SessionHandle::Resource(std::sync::Arc::new(FooSession { host })))
    }
}

struct FooSession {
    host: String,
}

#[async_trait]
impl ServiceSession for FooSession {
    fn server_info(&self) -> ServerInfo {
        ServerInfo { product: "Foo 3.1".into(), version: "3.1".into(), details: vec![("Host".into(), self.host.clone())] }
    }
    async fn ping(&self) -> Result<Duration> {
        let t = Instant::now();
        // ... round trip ...
        Ok(t.elapsed())
    }
    async fn close(&self) {}
}

#[async_trait]
impl ResourceSession for FooSession {
    async fn children(&self, parent: Option<&str>) -> Result<Vec<Node>> {
        Ok(match parent {
            None => vec![Node::new("db/main", "main", "database", NodeIcon::Database).folder()],
            Some(_) => vec![],
        })
    }

    async fn open(&self, node: &str, opts: OpenOptions) -> Result<Content> {
        let _ = (opts.limit, opts.offset, &opts.filter);
        Ok(Content::Text(format!("contents of {node}")))
    }

    fn actions(&self, kind: &str) -> Vec<ActionSpec> {
        match kind {
            "database" => vec![ActionSpec::new("drop", "Drop database").destructive()],
            _ => vec![],
        }
    }

    async fn run_action(&self, node: &str, action: &str, _args: Args) -> Result<ActionOutcome> {
        match action {
            "drop" => Ok(ActionOutcome::msg(format!("Dropped {node}")).refresh("")),
            _ => Err(Error::unsupported(format!("Unknown action {action}"))),
        }
    }
}
```

This skeleton compiles against the current `openvclient-core`. `NodeIcon` has these variants: `Server`, `Database`, `Collection`, `Index`, `Folder`, `File`, `Bucket`, `Topic`, `Queue`, `Exchange`, `Key`, `Table`, `Terminal`, `Info` and `User`. A `Content` can be `Table`, `TableWithNote`, `Json`, `Text`, `Props` or `Empty`.

### 3.4 SQL connectors

Implement `SqlSession` and a `SqlConsole` (one dedicated connection per editor tab):

```rust
#[async_trait]
pub trait SqlSession: ServiceSession {
    fn dialect(&self) -> Dialect;                    // openvclient_core::sqltext::Dialect
    fn capabilities(&self) -> SqlCapabilities;       // only set flags you really honour
    fn default_database(&self) -> Option<String>;
    async fn children(&self, node: Option<(ObjectKind, ObjPath)>) -> Result<Vec<TreeNode>>;
    async fn ddl(&self, kind: ObjectKind, path: &ObjPath) -> Result<String>;
    async fn columns(&self, path: &ObjPath) -> Result<Vec<ColumnInfo>>;
    async fn table_page(&self, q: TableQuery) -> Result<TablePage>;
    async fn count_rows(&self, path: &ObjPath, filter: Option<&str>) -> Result<u64>;
    async fn apply_changes(&self, path: &ObjPath, changes: Vec<RowChange>) -> Result<u64>;
    async fn catalog(&self, database: Option<&str>) -> Result<Catalog>;
    async fn open_console(&self, database: Option<String>) -> Result<Arc<dyn SqlConsole>>;
    async fn run(&self, database: Option<&str>, sql: &str) -> Result<u64>;
    // Optional, with default "unsupported" bodies: drop_object, truncate, create_database,
    // set_database_allow_connections, set_database_online, create_schema, activity, kill,
    // locks, metrics, principals, privileges, create_principal, drop_principal,
    // set_password, grant, privilege_names, backup, restore.
}

#[async_trait]
pub trait SqlConsole: Send + Sync {
    async fn execute(&self, statements: Vec<String>, opts: ExecOptions) -> Vec<StatementResult>;
    async fn explain(&self, sql: &str, analyze: bool) -> Result<ExplainResult>;
    fn cancel(&self);                                // callable from any thread mid-execute
    fn current_database(&self) -> Option<String>;
    async fn close(&self);
}
```

The UI hides everything whose `SqlCapabilities` flag is `false`. **Never set a flag you can't honour.** If an engine lacks a feature, return `Error::unsupported("…why, and what to use instead")`. SQLite's message for `EXPLAIN ANALYZE` is a good example.

If your engine speaks an existing wire protocol, reuse the existing crate. `PostgresDriver::flavor(...)` is how Kingbase and GaussDB are registered.

### 3.5 Errors

Map library errors onto `ErrorKind` (`Connection`, `Auth`, `Server`, `Unsupported`, `Invalid`, `Cancelled`, `Io`, `Other`). Put engine detail such as SQLSTATE or a server hint in `.with_detail(..)`. `Error::hint()` turns common failures into plain-language advice. If your engine has a distinctive error, extend `hint()` in `error.rs` rather than special-casing it in the UI. **Never put secrets in error messages or logs.**

### 3.6 Register it

1. Add the crate to `[workspace.dependencies]` in the root `Cargo.toml`.
2. In `crates/openvclient-app/Cargo.toml`, add an optional dependency and a feature (`foo = ["dep:openvclient-foo"]`), and add it to `default` once it's tested.
3. Register it in `crates/openvclient-app/src/main.rs` → `registry()` under `#[cfg(feature = "foo")]`.
4. Add the default port to `default_port()` in `crates/openvclient-app/src/conns.rs`, so tunnels work when the port is left empty.
5. Add a live test (see below), a row in `ROADMAP.md` and test cases in `docs/TEST_CASES.md`.

### 3.7 Live test

Put it next to the connector (as `tests.rs` or a `#[cfg(test)] mod tests`). **Skip when the server is unreachable**, so `cargo test` passes on any machine:

```rust
#[tokio::test]
async fn live_foo() {
    let Ok(h) = FooDriver::default().connect(params()).await else {
        eprintln!("skipping: no Foo reachable");
        return;
    };
    // create a scratch database, exercise browse / query / edit / drop, clean up
}
```

Use a high, unusual host port (5xxxx) for the test container, and allow overrides through `OPENVCLIENT_FOO_*` environment variables. Add the container to the CI service list in `.github/workflows/ci.yml`.

---

## 4. Coding conventions

### Rust
- Run `cargo fmt --all` before you commit.
- Run `cargo clippy --workspace --all-targets -- -D warnings`. CI runs this exact command.
- Write async code with `async-trait` and tokio. Don't block the UI thread. The app runs I/O on the background runtime (`bg.rs`) and polls the results once per frame.
- Return `openvclient_core::Result`. Don't `unwrap()` on anything that comes from the network or the user.
- Log with `tracing` under target `openvclient`. Never log secrets. Connection logs carry host, port and outcome only.
- Use rustls or the platform TLS stacks. Don't add a dependency that needs a system library on every platform without discussing it first.

### UI (egui)
- **Read `design-system/openvclient/MASTER.md` first.** A page file in `design-system/openvclient/pages/` overrides it for that view.
- Use the `ui-ux-pro-max` skill (`.claude/skills/ui-ux-pro-max`) for all UI design work. The skill has no egui stack, so use its `--design-system` and `--domain ux|style|color|typography` searches and map the results to egui yourself.
- **Colours come from `theme::tokens(ui)`** (roles such as `bg`, `card`, `fg`, `muted_fg`, `accent`, `destructive`, `warning`, `info`). **Never write raw hex or `Color32::from_rgb` in views.** If you need a new role, add it to `theme.rs` for both themes and keep a 4.5:1 contrast ratio.
- **Icons come from `egui_material_icons`** (`egui_material_icons::icons::*`, combined with text through `theme::ico`). **Never use emoji as icons.**
- Use the spacing and sizes in `theme::space` and `theme::icon`. Keep pointer targets at least 26 px tall.
- Destructive actions go through the confirmation flow (`cx.confirm`), which applies the production-environment safety rail.
- Only offer an action when the session's capabilities say it's supported.

---

## 5. Testing

### 5.1 Unit tests
```sh
cargo test --workspace
```
Pure tests (SQL splitting, EXPLAIN parsing, CSV and SQL export, fuzzy palette matching, autocomplete) always run. Connector tests connect to live servers and **skip themselves** (`eprintln!("skipping: …")`) when a server is unreachable. Add `-- --nocapture` to see which tests skipped.

### 5.2 Live test services

| Service | Expected endpoint | Credentials / env | Start it |
|---|---|---|---|
| PostgreSQL | `localhost:5432` | user `$USER`, no password. Override with `OPENVCLIENT_PG_HOST`, `OPENVCLIENT_PG_USER`, `OPENVCLIENT_PG_PASSWORD` | `docker run -d --rm --name ovc-pg -p 5432:5432 -e POSTGRES_USER=$USER -e POSTGRES_HOST_AUTH_METHOD=trust postgres:16` |
| MySQL | `127.0.0.1:3306` | `root`, no password. Override with `OPENVCLIENT_MY_HOST`, `OPENVCLIENT_MY_PORT`, `OPENVCLIENT_MY_USER`, `OPENVCLIENT_MY_PASSWORD` | `docker run -d --rm --name ovc-mysql -p 3306:3306 -e MYSQL_ALLOW_EMPTY_PASSWORD=yes mysql:8` |
| Redis | `127.0.0.1:56379` | none (`OPENVCLIENT_REDIS_PORT`) | `docker run -d --rm --name openvclient-test-redis -p 56379:6379 redis:7-alpine` |
| SQL Server | `127.0.0.1:51433` | `sa` / `OpenVClient#2026` (`OPENVCLIENT_MS_PORT`, `OPENVCLIENT_MS_PASSWORD`) | `docker run -d --rm --name ovc-mssql -p 51433:1433 -e ACCEPT_EULA=Y -e 'MSSQL_SA_PASSWORD=OpenVClient#2026' mcr.microsoft.com/mssql/server:2022-latest` |
| MongoDB | `127.0.0.1:57017` | `root` / `ovcpass` | `docker run -d --rm --name ovc-mongo -p 57017:27017 -e MONGO_INITDB_ROOT_USERNAME=root -e MONGO_INITDB_ROOT_PASSWORD=ovcpass mongo:7` |
| ClickHouse | `127.0.0.1:58123` (HTTP) | `ovc` / `ovcpass` | `docker run -d --rm --name ovc-ch -p 58123:8123 -e CLICKHOUSE_USER=ovc -e CLICKHOUSE_PASSWORD=ovcpass -e CLICKHOUSE_DEFAULT_ACCESS_MANAGEMENT=1 clickhouse/clickhouse-server` |
| Elasticsearch | `127.0.0.1:59200` | no auth | `docker run -d --rm --name ovc-es -p 59200:9200 -e discovery.type=single-node -e xpack.security.enabled=false -e ES_JAVA_OPTS='-Xms512m -Xmx512m' elasticsearch:8.15.0` |
| RabbitMQ (in development) | `127.0.0.1:55673` (management API) | `ovc` / `ovcpass` (`OPENVCLIENT_RABBIT_PORT`, `_USER`, `_PASSWORD`) | `docker run -d --rm --name ovc-rabbit -p 55673:15672 -e RABBITMQ_DEFAULT_USER=ovc -e RABBITMQ_DEFAULT_PASS=ovcpass rabbitmq:3-management` |
| S3 / MinIO (in development) | `http://127.0.0.1:59000` | `ovcadmin` / `ovcpass123`, path-style (`OPENVCLIENT_S3_ENDPOINT`) | `docker run -d --rm --name ovc-minio -p 59000:9000 -e MINIO_ROOT_USER=ovcadmin -e MINIO_ROOT_PASSWORD=ovcpass123 minio/minio server /data` |
| Docker | local daemon socket | — | Docker Desktop or dockerd. Container checks use the `openvclient-test-redis` container above. |
| Network paths | SOCKS5 `127.0.0.1:51080` (`puser`/`ppass`), HTTP proxy `127.0.0.1:53128`, SSH `127.0.0.1:52222` (`tunnel`/`tunnelpw`, optional key in `OPENVCLIENT_TEST_SSH_KEY`) | — | The proxies must reach a Redis container named `redis` on port 6379 through a shared docker network. |

The connector tests create and drop their own scratch databases (`openvclient_test`, `ovc_test`). Don't point them at a server whose data you care about.

For backup and restore tests, `mysqldump`/`mysql` and `pg_dump`/`pg_restore` must be on `PATH`. If they aren't, the backup capability is reported as off and that part of the test is skipped.

### 5.3 Headless UI walkthrough

This test drives the **real app** headlessly (egui_kittest + wgpu) against the local services and writes PNG screenshots. Use it to check layout instead of OS screen capture, which needs Screen Recording permission on macOS.

```sh
cp -r "<your config dir>" /tmp/ovc-ws        # a copy of a workspace with the demo profiles
OPENVCLIENT_CONFIG_DIR=/tmp/ovc-ws OPENVCLIENT_SHOTS=/tmp/ovc-shots \
  cargo test -p openvclient-app ui_walkthrough -- --ignored --nocapture
```
- It is `#[ignore]` by default because it needs seeded services and a workspace.
- Demo data: `demo/shop.db` (SQLite) and a demo database on the local Postgres. The test reads `OPENVCLIENT_DEMO_DB` and falls back to `openv_demo` if it isn't set.
- Large and strong text can look faint in kittest renders even though it looks fine in the real window. Don't chase that.

### 5.4 Manual test catalogue
[docs/TEST_CASES.md](docs/TEST_CASES.md) lists the manual and automated test cases with IDs. When you add a feature, add its cases there, and name the automated test if one exists.

---

## 6. Commits and pull requests

- **Branch** from `main` and keep each PR focused on one connector, one feature or one fix.
- **Commit messages**: use a short imperative subject of about 72 characters or fewer, with an optional scope, for example `postgres: show partitioned tables` or `ui: keyboard focus ring in grid`. Use the body to explain *why*.
- Before you push:
  ```sh
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  ```
- For a UI change, attach before and after screenshots (the walkthrough PNGs are fine) and say which theme or themes you checked.
- For a connector change, say which server versions you tested against live.
- Update `ROADMAP.md`, `README.md` and `docs/TEST_CASES.md` when the change affects them. **Don't claim support you haven't tested.**
- A maintainer reviews the PR. CI must pass on Linux, macOS and Windows. PRs are squash-merged.

There is no DCO and no CLA. Your contribution is licensed under MIT OR Apache-2.0, as described in the [README](README.md#license).

## 7. Code of conduct

This project follows the [Contributor Covenant 2.1](CODE_OF_CONDUCT.md). Be kind, assume good faith, and report problems to the maintainers.
