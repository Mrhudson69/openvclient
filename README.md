# OpenVClient

**One open-source desktop app for your databases and infrastructure.**

OpenVClient is a native, GPU-rendered desktop client written in Rust with [egui](https://github.com/emilk/egui). Use it to browse, query, edit, monitor and administer relational databases, document stores, key-value stores, search engines and container hosts. It runs on macOS, Windows and Linux.

Every connector is free and open source. There are no premium tiers, no account and no telemetry.

> **Status: pre-release (0.1).** Expect rough edges. The project follows one rule: *never pretend a feature is supported.* The UI only offers an action when the connector really implements it, and this README only lists features that exist in the code today.

---

## Screenshots

> _Placeholder. Screenshots will be added before the first public release._
>
> To generate them yourself, use the headless UI walkthrough (see [CONTRIBUTING.md](CONTRIBUTING.md#headless-ui-walkthrough)). It writes PNGs of the home screen, the SQL editor, the table grid, the plan diagram and both themes.

| Home | SQL editor | Execution plan |
|---|---|---|
| _coming soon_ | _coming soon_ | _coming soon_ |

---

## Features

### Connections
- Connection profiles organised in **nested folders**. You can create, rename, move, duplicate and delete profiles and folders.
- **Environment tags** (development, staging, production…). Production connections can require you to type the name before a destructive action runs (Settings → Safety).
- A **read-only** flag for each profile.
- **Test connection** before saving.
- Passwords and other secrets are stored in the **OS keychain**, never in the workspace file.
- **Network paths** for every TCP connector: direct, **SSH tunnel** (password, private key with passphrase, or SSH agent), **SOCKS5** proxy (optional auth) and **HTTP CONNECT** proxy (optional auth).
- The SSH host key is **pinned on first use**. If the key changes later, the connection is refused with a clear explanation.

### SQL databases
- An object tree with databases, schemas, tables, views, materialized views, sequences, functions, procedures, triggers and events. Each engine shows what it supports; SQL Server gets an SSMS-style layout.
- **SQL editor** with syntax highlighting and autocomplete (keywords, objects, columns, including after table aliases). You can run the statement under the cursor or the selection (⌘/Ctrl+Enter), run the whole script (⌘/Ctrl+Shift+Enter or F5) and **cancel** a running query (⌘/Ctrl+.). The script splitter understands dialect details such as PostgreSQL `$$` bodies, MySQL `DELIMITER` and T-SQL `GO`.
- **EXPLAIN / EXPLAIN ANALYZE** (⌘/Ctrl+E) where the engine supports it, with a **graphical plan diagram** in the style of SQL Server Management Studio.
- A **table browser** with server-side filtering, sorting, paging and row counts. You can **edit cells, insert rows and delete rows**, and changes apply in one transaction. Editing works when rows can be addressed by a primary key or a physical row id. Otherwise the grid is read-only and says why.
- **Export** results to CSV, TSV, JSON, SQL `INSERT` or Markdown. **Import** CSV into a table.
- **Structure and DDL** view for every object.
- **Administration**, where the engine supports it: create and drop databases and schemas, truncate, live **monitoring** (metrics, sessions, locks; kill a query or session), and **users, roles and privileges** (create, drop, change password, grant, revoke).
- **Backup and restore**:
  - SQLite: built-in online backup.
  - PostgreSQL: `pg_dump` / `pg_restore`.
  - MySQL and MariaDB: `mysqldump` / `mariadb-dump` and the `mysql` client.
  - SQL Server: `BACKUP` / `RESTORE` to files on the server.

  The external tools must be on your `PATH`. If they are missing, the backup actions are hidden.

### Other services
- **Redis**: database list, `SCAN` browsing with type filter, a viewer and editor for every key type (string, hash, list, set, zset, stream), TTL, rename, bulk delete, a command console, `INFO`, the client list (with kill) and the slow log.
- **MongoDB**: databases, collections, views and indexes. You can filter and page documents and insert, update or delete them; create and drop collections and indexes; import and export JSON; and run database commands in a console.
- **Elasticsearch**: indices with open, close, refresh and delete, plus document search, index and delete, delete-by-query and a REST console.
- **ClickHouse**: a SQL connector over the HTTP interface, with the editor, the table browser and `EXPLAIN`.
- **Docker**: containers (start, stop, restart, pause, unpause, kill, remove), logs, inspect, live stats and `exec`. Images (pull, remove), volumes, networks and system info.

### Workspace
- A tabbed and tiled workspace.
- A **command palette** (⌘/Ctrl+K or ⌘/Ctrl+Shift+P) with fuzzy search over connections, objects and actions.
- Keyboard shortcuts: new connection ⌘/Ctrl+N, new SQL tab ⌘/Ctrl+T, close tab ⌘/Ctrl+W, home ⌘/Ctrl+Shift+H.
- **Toasts with plain-language error hints**, for example "nothing is listening on that host and port" or "the SSH server's identity changed". You can copy the message, and the full diagnostics (without secrets) are written to the log.
- **Dark, light or system theme**, editor font size, scroll speed, maximum rows per result and table page size.

---

## Supported connectors

A connector is marked ✅ only when it is implemented **and** tested against a live server. See [ROADMAP.md](ROADMAP.md) for details.

| Service | Family | Client library | Status |
|---|---|---|---|
| MySQL | SQL | mysql_async | ✅ |
| MariaDB | SQL | mysql_async (MariaDB mode) | ✅ |
| PostgreSQL | SQL | tokio-postgres | ✅ |
| SQLite | SQL | rusqlite (bundled) | ✅ |
| SQL Server / Azure SQL | SQL | tiberius (TDS, pure Rust) | ✅ |
| ClickHouse | SQL (analytical) | HTTP interface | ✅ |
| MongoDB | Documents | mongodb | ✅ |
| Redis | Key-value | redis | ✅ |
| Elasticsearch | Search | REST | ✅ |
| Docker | Containers | bollard | ✅ |
| Kingbase | SQL | PostgreSQL wire protocol | 🟡 registered, untested |
| GaussDB / openGauss | SQL | PostgreSQL wire protocol | 🟡 registered, untested |
| RabbitMQ | Messaging | Management HTTP API | ✅ |
| SSH: terminal, SFTP, remote commands | Remote | russh / russh-sftp | ✅ |
| FTP / FTPS (explicit and implicit TLS) | Files | suppaftp (rustls) | ✅ |
| Amazon S3 and S3-compatible: Backblaze B2, IDrive e2, Wasabi, Cloudflare R2, DigitalOcean Spaces, Google Cloud Storage (HMAC), Oracle, Linode, Scaleway, Hetzner, Storj, MinIO, Alibaba OSS | Object storage | S3 REST (SigV4) | ✅ S3 tested against SeaweedFS; providers are presets of the same connector |
| Azure Blob Storage / ADLS Gen2 | Object storage | Blob REST (Shared Key, SAS, Entra ID) | ✅ tested against Azurite |
| Kafka, Cassandra | various | — | ⏳ next |
| Oracle, Db2, JDBC | SQL | need vendor client libraries | ⏳ planned, **not available yet** |

### SSH authentication

Password · keyboard-interactive (PAM / OTP / 2FA prompts) · private key file (with passphrase) · pasted private key · OpenSSH user certificate · ssh-agent / Pageant / Windows OpenSSH agent · `none`. Jump hosts (ProxyJump) are supported. Host keys are checked against `~/.ssh/known_hosts` first, then pinned on first use. Kerberos/GSSAPI is not supported yet.

### Azure Blob Storage authentication

Account name + key · connection string (including `UseDevelopmentStorage=true`) · SAS URL or token (account or container level) · Microsoft Entra ID: interactive device-code sign-in, service principal, managed identity, default chain (env vars → managed identity → Azure CLI), user + password · anonymous public containers · Azurite emulator. Public, China and US Government clouds.

### SQL Server authentication

| Method (`auth`) | Notes |
|---|---|
| `sql` | SQL Server login (default) |
| `windows` | Windows/NTLM user (`DOMAIN\user`). **Windows builds only.** |
| `integrated` | Current Windows login (SSPI). **Windows builds only.** |
| `entra_password` | Microsoft Entra ID user and password |
| `entra_service_principal` | App registration: client ID and secret |
| `entra_managed_identity` | System-assigned or user-assigned managed identity |
| `entra_interactive` | Device-code sign-in. The code is shown in the app. |
| `entra_default` | Environment variables (`AZURE_TENANT_ID`/`AZURE_CLIENT_ID`/`AZURE_CLIENT_SECRET`), then managed identity, then Azure CLI (`az login`) |
| `entra_token` | A raw access token that you paste |

Named instances (for example `SQLEXPRESS`) are resolved through **SQL Server Browser** when the port is left empty.

---

## Install

Installers are produced by the release workflow (see [docs/RELEASE_PLAN.md](docs/RELEASE_PLAN.md)); you can also build them yourself:

| Platform | Command | Result |
|----------|---------|--------|
| macOS | `./scripts/package-macos-dmg.sh` | `dist/OpenVClient-<version>-macos-<arch>.dmg`: open it and drag OpenVClient onto Applications |
| Windows | `.\scripts\bundle-windows.ps1` (needs [Inno Setup 6](https://jrsoftware.org/isinfo.php)) | `dist\OpenVClient-<version>-windows-x64-setup.exe`: a setup wizard (licence, per-user or all users, folder, shortcuts, launch) plus a portable zip |
| Linux | `./scripts/bundle-linux.sh` | `.deb`, `.rpm`, AppImage and a tar.gz with `install.sh`; all add a menu entry and icon |

Builds that are not signed with an Apple Developer ID / Windows code-signing certificate show a first-run warning (macOS: right-click → Open; Windows SmartScreen: More info → Run anyway). The release workflow signs and notarizes when the signing secrets are configured.

Building on a fresh Windows or Linux machine with Claude Code: see [docs/WINDOWS_BUILD_PROMPT.md](docs/WINDOWS_BUILD_PROMPT.md) and [docs/LINUX_BUILD_PROMPT.md](docs/LINUX_BUILD_PROMPT.md).

To build from source:

### Prerequisites (all platforms)
- **Rust 1.95 or newer** (edition 2024). Install it with [rustup](https://rustup.rs).
- A GPU driver with OpenGL 3.x or newer. The app renders with `glow` (OpenGL).
- Optional: `pg_dump`/`pg_restore`, `mysqldump`/`mariadb-dump`/`mysql` on `PATH` for backup and restore. The Azure CLI (`az`) is used by `entra_default`.

### macOS
```sh
xcode-select --install          # Command Line Tools (clang, codesign)
git clone https://github.com/Mrhudson69/openvclient.git
cd openvclient
cargo run --release             # or build the app bundle:
./scripts/bundle-macos.sh       # -> dist/OpenVClient.app (ad-hoc signed)
./scripts/package-macos-dmg.sh  # -> drag-to-Applications .dmg
```
OpenVClient needs macOS 11 or later. TLS for PostgreSQL, MySQL and Redis uses the system Security framework.

### Windows
1. Install the [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **Desktop development with C++** workload (MSVC and the Windows SDK).
2. Install Rust with `rustup-init.exe` (the default `x86_64-pc-windows-msvc` toolchain).
3. Build:
   ```powershell
   git clone https://github.com/Mrhudson69/openvclient.git
   cd openvclient
   cargo run --release
   # or: .\scripts\bundle-windows.ps1   -> setup.exe installer + portable zip in dist\
   ```
Windows builds are the only builds that support SQL Server `windows` and `integrated` authentication (SSPI). TLS uses SChannel.

### Linux
Install the build dependencies first. eframe and winit need X11 and Wayland headers. PostgreSQL, MySQL and Redis TLS uses OpenSSL (through `native-tls`), so the OpenSSL headers are required.

**Debian / Ubuntu**
```sh
sudo apt-get install -y build-essential pkg-config libssl-dev \
  libxkbcommon-dev libwayland-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
  libx11-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev
```
**Fedora**
```sh
sudo dnf install -y gcc pkg-config openssl-devel libxkbcommon-devel wayland-devel \
  libxcb-devel libX11-devel libXcursor-devel libXrandr-devel libXi-devel mesa-libGL-devel
```
**Arch**
```sh
sudo pacman -S --needed base-devel pkgconf openssl libxkbcommon wayland libxcb libx11 mesa
```
Then build:
```sh
git clone https://github.com/Mrhudson69/openvclient.git
cd openvclient
cargo run --release
# or: ./scripts/bundle-linux.sh    -> dist/openvclient-<version>-linux-<arch>.tar.gz
```
Some Linux runtime services are needed at runtime only, not to build:
- **Secret Service** (GNOME Keyring, KWallet ≥ 5.97 or KeePassXC) on the session D-Bus, to store passwords. The keyring backend talks to D-Bus in pure Rust, so `libdbus` is not needed.
- **xdg-desktop-portal** (with a GTK, KDE or GNOME backend) for the native open and save dialogs.

### Build without some connectors
Every connector is a cargo feature of `openvclient-app`. The defaults are `sqlite postgres mysql redis docker sqlserver mongodb http`.
```sh
cargo run --release --no-default-features --features "sqlite,postgres"
```

---

## Running

```sh
cargo run --release           # the binary is named `openvclient`
```

| Variable | Purpose |
|---|---|
| `OPENVCLIENT_CONFIG_DIR` | Use a different configuration and workspace directory (useful for testing). |
| `OPENVCLIENT_LOG` | Log filter, `tracing` `EnvFilter` syntax. Default `warn,openvclient=info`. Example: `OPENVCLIENT_LOG=debug`. |

### Configuration directory

All profiles, folders and settings live in `workspace.json` in this directory. Secrets are kept in the keychain instead.

| OS | Location |
|---|---|
| macOS | `~/Library/Application Support/dev.OpenVClient.OpenVClient/` |
| Windows | `%APPDATA%\OpenVClient\OpenVClient\config\` |
| Linux | `$XDG_CONFIG_HOME/openvclient/` (usually `~/.config/openvclient/`) |

Settings → *Workspace file* shows the exact path on your machine. If `workspace.json` cannot be read, OpenVClient keeps a copy as `workspace.broken-<timestamp>.json` and starts with an empty workspace. It never overwrites the broken file silently.

### Logs
Logs go to stderr and to `<config dir>/logs/openvclient.log`. The file is rotated to `openvclient.1.log` at 5 MB. Settings → **Open logs folder** opens the directory. Logs record host, port and outcome for each connection. They **never contain secrets**.

---

## Security model

- **Secrets stay in the OS keychain.** This is macOS Keychain, Windows Credential Manager or the Linux Secret Service. Entries use the service name `dev.openvclient.client`, keyed by the profile id. `workspace.json` holds no passwords, tokens or passphrases. SSH passwords, key passphrases and proxy passwords are stored the same way.
- **SSH host keys are never trusted silently.** A key listed in `~/.ssh/known_hosts` is accepted; any other new key is shown with its SHA-256 fingerprint and only trusted when you confirm it, then pinned in the profile. A key that changes later is refused with an explanation.
- **Workspace data at rest:** SQL history and connection URIs with embedded passwords are encrypted (AES-256-GCM) in `workspace.json`, with the key held in the system keychain; the file is owner-only on macOS/Linux.
- **TLS.** SQL Server uses rustls. Encryption is on by default; `required` also verifies the certificate. PostgreSQL, MySQL and Redis use the platform TLS stack, with each engine's usual SSL modes.
- **Safety rails.** Destructive actions ask for confirmation. Profiles tagged *production* can require you to type the connection name. Read-only profiles refuse changes.
- **No telemetry.** OpenVClient makes no network connections except to the servers you configure, plus Microsoft Entra ID endpoints when you choose an Entra auth method. There is no auto-update or analytics.

To report a vulnerability, see [SECURITY.md](SECURITY.md).

---

## Contributing

Contributions are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) covers the workspace layout, adding a connector, the UI conventions and running the live tests. The manual and automated test catalogue is in [docs/TEST_CASES.md](docs/TEST_CASES.md). Please follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## License

OpenVClient is dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution you intentionally submit for inclusion in the work, as defined in the Apache-2.0 license, shall be dual-licensed as above, without any additional terms or conditions.
