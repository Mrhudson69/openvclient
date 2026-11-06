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

See [TEST_CASES.md](TEST_CASES.md) for the full test catalogue.
