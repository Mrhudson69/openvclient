# Changelog

All notable changes to OpenVClient are listed here. The project follows
[Semantic Versioning](https://semver.org); pre-releases are tagged
`vX.Y.Z-beta.N` and published as GitHub pre-releases.

## [1.0.0-beta.1] — 2026-11-06

First public beta. Full notes: [docs/release-notes/v1.0.0-beta.1.md](docs/release-notes/v1.0.0-beta.1.md).

### Added
- Connectors: PostgreSQL, MySQL, MariaDB, SQL Server / Azure SQL (SQL, Windows, integrated and Entra ID sign-in), SQLite, ClickHouse, Kingbase and GaussDB presets, MongoDB, Redis, Elasticsearch/OpenSearch, RabbitMQ, Docker, SSH (terminal, SFTP, commands), FTP/FTPS, Amazon S3 and 13 S3-compatible providers, Azure Blob Storage.
- SSH tunnels (with jump hosts), SOCKS5 and HTTP CONNECT proxies for TCP connectors.
- SQL editor, SSMS-style graphical execution plans, results grid with editing, import/export, object explorer, users and privileges, monitoring, backup and restore.
- Folders, environments, ⌘K/Ctrl+K search, dark and light themes, in-app update check.
- Installers: macOS drag-to-Applications DMG, Windows setup wizard and portable zip, Linux .deb, .rpm, AppImage and tar.gz.

### Security
- Secrets only in the OS credential store; SQL history and URIs with passwords encrypted at rest.
- SSH host keys verified against known_hosts or confirmed by the user before first use.
- See docs/SECURITY_AUDIT.md for the audit and fixes.

[1.0.0-beta.1]: https://github.com/Mrhudson69/openvclient/releases/tag/v1.0.0-beta.1
