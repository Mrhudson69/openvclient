# QA report

Date: 2026-11-06. Build: `main` (macOS arm64 release bundle).
Method: live connector test suites against local servers (Docker), the headless
`ui_walkthrough` screenshot run, manual review of every screen, the security
review in [SECURITY_AUDIT.md](SECURITY_AUDIT.md), and `cargo clippy -D warnings`.

Test servers used: PostgreSQL 16, MySQL 26, MariaDB 11, SQL Server 2022, Redis 7,
MongoDB 7, ClickHouse, Elasticsearch 8.15, RabbitMQ 3, OpenSSH (linuxserver),
SOCKS5 and Squid proxies, SeaweedFS (S3 API), Azurite (Azure Blob), Alpine FTP.

## Fixed in this round

| ID | Area | Bug | Fix |
|----|------|-----|-----|
| QA-01 | New connection | Picker overflowed the window: no scrolling, close button pushed off-screen, oversized icons, grouped by category | Flat, searchable, scrollable grid of compact tiles; header and close button always visible; Esc closes |
| QA-02 | Search (⌘K) | Scrolling lagged and rows overflowed their width | Virtualized rows (`show_rows`), scroll-to-selection only on keyboard moves, detail text clipped to the row |
| QA-03 | Folders | No way to rename a folder or move a connection between folders | "Rename folder…" (renames subfolders too) and "Move to folder…" |
| QA-04 | Home | Clicking a connector chip opened the generic picker | Opens the form for that connector directly |
| QA-05 | SQL Server | Only SQL logins were possible | SQL, Windows/NTLM, integrated, and Entra ID (password, service principal, managed identity, device code, default chain, raw token); named instances via SQL Browser |
| QA-06 | Errors | Connection failures showed raw driver text only | Plain-language hint under every error, "Copy diagnostics", per-connection log lines in `<config>/logs/openvclient.log` |
| QA-07 | Plan view | Statement header took half the panel; row-count labels clipped by boxes | One-line truncated header; wider gaps, centred labels |
| QA-08 | SFTP | Uploading a new file failed with "No such file" | Create-then-write |
| QA-09 | Connections | With "Save password" off, saving the dialog deleted the SSH/proxy secrets it had just stored (audit F-16) | Only the main password is removed |
| QA-10 | Restore | Plain SQL dumps could run shell commands through psql `\!` / mysql `system` (F-04) | Dumps are scanned and refused, with the offending line |
| QA-11 | Backup | Database names were passed to `pg_dump`/`mysqldump` as raw arguments (F-03) | Quoted as a connection string / after `--` |
| QA-12 | SQL Server | Default encryption trusted any certificate (F-01) | New connections verify; "on" (trust) is an explicit opt-in |
| QA-13 | SQL Server | Read-only connections still ran writes typed in the editor (F-05) | Data-changing keywords are refused on read-only connections |
| QA-14 | Logging | Passwords inside connection URIs could reach logs, toasts and diagnostics (F-07) | URL passwords are redacted everywhere text leaves the app |
| QA-15 | Files | `workspace.json` and the log file were world-readable (F-11) | Owner-only permissions on Unix |
| QA-16 | SSH | `~/.ssh/known_hosts` was ignored (F-09, part) | Checked first; a mismatch is refused with instructions |
| QA-17 | Build | 80 clippy warnings and an unformatted tree would fail CI | Clean under `clippy -D warnings` and `rustfmt --check` |
| QA-18 | Walkthrough | The screenshot test pointed at a database that does not exist | Uses `openv_demo`, override with `OPENVCLIENT_DEMO_DB` |

## Open bugs and gaps

| ID | Severity | Area | Description |
|----|----------|------|-------------|
| QA-20 | Medium | PostgreSQL / MySQL TLS | `prefer`/`require` do not verify the server certificate, and `verify-ca` maps to no verification (audit F-02) |
| QA-21 | Medium | Read-only mode | Redis `CONFIG`/`SCRIPT`, Mongo `aggregate` with `$out`, and Elasticsearch paths that merely contain `_search` can still write (F-06) |
| QA-22 | Low | Network paths | When a MongoDB `uri` or HTTP `url` override is set, the configured SSH/proxy tunnel is bypassed silently (F-08) |
| QA-23 | Low | Tunnels | Any local process can use the 127.0.0.1 forwarder while a tunnel is open (F-10) |
| QA-24 | Low | SQL Server | The table WHERE filter can be bypassed without a `;` (F-12) |
| QA-25 | Low | ClickHouse | Identifier quoting does not escape backslashes (F-13, needs confirmation) |
| QA-26 | Low | SSH | First connection to an unknown host is trusted without a prompt (TOFU) |
| QA-27 | Low | Export | CSV export does not neutralise spreadsheet formulas (F-19) |
| QA-28 | Low | Large files | FTP, SFTP and Azure uploads/downloads hold the whole file in memory; Azure uploads over 4 GiB and S3 uploads over 5 GiB are refused (no block/multipart upload yet) |
| QA-29 | Low | Terminal | No mouse selection inside the terminal (use "Copy screen"); no mouse reporting for full-screen programs |
| QA-30 | Info | HTTP connectors | Response bodies are read without a size limit (F-18) |
| QA-31 | Info | SQL Server | Existing profiles without an explicit Encryption value now verify certificates; self-signed servers need "on" |
| QA-32 | Gap | Connectors | Kafka, Cassandra, Oracle, Db2 and JDBC are not implemented. Kingbase and GaussDB are registered but untested |
| QA-33 | Gap | SSH | Kerberos/GSSAPI login is not supported |
| QA-34 | Gap | SQL Server | Windows/integrated auth only exists in the Windows build and has not been run there yet |
| QA-35 | Gap | Updates | The update check works, but no GitHub release exists yet, so it always reports "latest" |
| QA-36 | Gap | Tests | The terminal tab and the Entra ID flows have no automated end-to-end test (need an interactive session / a tenant) |

## Windows round (2026-11-06)

Machine: Windows 11 Pro 26200 in a QEMU VM over Remote Desktop (no GPU: Microsoft
Basic Display Adapter, 1680x1050, 100 % scaling). Rust stable MSVC, VS 2022 Build
Tools, Inno Setup 6.7.3. No Docker and no local database servers, so every live
connector test skipped itself.

### Fixed in this round

| ID | Area | Bug | Fix |
|----|------|-----|-----|
| QA-40 | Start-up | The app exited immediately on any Windows machine without OpenGL 2+ (Remote Desktop, VMs, old drivers), with nothing but a log line, because the release build has no console | Falls back to `wgpu` on Direct3D 12 (WARP software rendering) after `glow` fails; if both fail, a message box explains why |
| QA-41 | Terminal | Ctrl+C and Ctrl+X never reached the shell: egui-winit turns them into Copy/Cut events on Windows and Linux, so a runaway command could not be interrupted | Those events are sent to the shell as 0x03 / 0x18 outside macOS; "Copy screen" still copies |
| QA-42 | Installer | Upgrading over a running OpenVClient stopped with "unable to automatically close all applications" and needed the user to close it by hand | The window agrees to `WM_QUERYENDSESSION` and closes itself on `WM_ENDSESSION`, and Setup uses `CloseApplications=force`; a plain `RmShutdown` is cancelled by Windows whatever the app answers. Measured: silent upgrade exit 0, app closed gracefully, workspace and history intact |
| QA-43 | Window | The 1440x900 start size and 1100x700 minimum do not fit a 1366x768 laptop or 1920x1080 at 150 %; Windows also cascades new windows, pushing the bottom under the taskbar | The first window is centred, the minimum is lowered to what the monitor holds, and an oversized window starts maximized |
| QA-44 | Backup | `pg_dump`/`mysqldump` were only found on `PATH`, but the Windows installers do not add their versioned `bin` folders to it | Also searched under Program Files (`PostgreSQL\<v>`, `MySQL\MySQL Server <v>`, `MariaDB <v>`), newest first and scanned once |
| QA-45 | Backup | Each external tool flashed a console window, because the release build has no console of its own | Console tools are started with `CREATE_NO_WINDOW` |
| QA-46 | Entra ID | `entra_default` could never find the Azure CLI: on Windows `az` is `az.cmd`, which `Command::new` does not resolve | The `.cmd` is named explicitly |
| QA-47 | SSH | With no agent running, agent auth failed with "SSH agent: early eof", because `connect_pageant()` succeeds even when Pageant is absent | Plain-language error naming the ssh-agent service and Pageant |
| QA-48 | SSH | Key paths starting with `~` were not expanded, since Windows has no `HOME` | `USERPROFILE` is used, and `~\` is accepted alongside `~/` |
| QA-49 | Settings | "Open logs folder" opened the Documents folder when the path had forward slashes, and did nothing when the folder did not exist yet | Separators are normalised for Explorer and the folder is created first |
| QA-50 | Docker | A missing named pipe gave the bare message "client error (Connect)" | Windows hint naming Docker Desktop and `\.\pipe\docker_engine` |
| QA-51 | Installer | The wizard used two stretched 24-bit BMPs and was light-only | One PNG per DPI step (100-250 %) and `WizardStyle=modern dynamic`, so it follows the Windows light/dark setting |
| QA-52 | Installer | The License page showed only the MIT text although the project is MIT OR Apache-2.0 | Both texts, with a note that either may be chosen |
| QA-53 | Installer | A pre-release version such as `0.2.0-rc.1` broke the version resource, which takes digits only | The numeric part is used for `VersionInfoVersion`, the full string for the product text |
| QA-54 | Packaging | `bundle-windows.ps1` shipped an exe without icon or version resource when `rc.exe` was missing, and needed PowerShell 7 | The build fails instead (`OPENVCLIENT_REQUIRE_RESOURCES`), the resource is verified after the build, and the script runs under PowerShell 5.1 too |
| QA-55 | CI | Windows-only code was never linted (clippy runs on Linux) and the installer script was never compiled | The Windows CI job runs clippy and compiles the `.iss`, with a pinned, hash-checked Inno Setup |

### Verified by hand on Windows

Window with icon in title bar and taskbar and no console window; Settings and both
connection dialogs at 1440x900 and near the minimum size; passwords saved to
Credential Manager (`<profile-id>.dev.openvclient.client`) with `workspace.json`
holding only `ovc-sealed:v1:` ciphertext for SQL history; SQLite browse, query
(Ctrl+T, Ctrl+Enter) and CSV export through the native save dialog; "Open logs
folder" in Explorer; the SSH agent pipe listing a loaded key; the full installer
wizard per-user, including upgrade over an existing install and silent uninstall
keeping user data; the headless walkthrough at 1440x900 and 1100x600.

### Not tested on Windows

| ID | Area | Why |
|----|------|-----|
| QA-60 | SQL Server `windows` / `integrated` auth | No reachable SQL Server. These paths exist only in the Windows build (tiberius `winauth`), so they remain untested (see QA-34) |
| QA-61 | All live connectors | No Docker and no local servers on this machine; every connector test skipped. Linux CI still runs them |
| QA-62 | SSH terminal, host-key trust prompt, tunnels | No reachable SSH server. The agent pipe and Pageant paths were tested directly; typing, Ctrl+C, resize and the trust prompt were not |
| QA-63 | Install for all users | Only the per-user install was walked through; the all-users path needs an admin account |
| QA-64 | HiDPI | The test machine runs at 100 % scaling, so 150 % was not checked on real hardware (the walkthrough renders at 1.5x) |
| QA-65 | GPU rendering | This machine has no GPU, so only the Direct3D/WARP fallback was exercised, never the normal `glow` path |

See [TEST_CASES.md](TEST_CASES.md) for the full test catalogue.
