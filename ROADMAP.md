# OpenVClient connector roadmap

Every connector is free and open source, with no premium tiers. A box is ticked only when the connector is implemented **and** tested against a live server.

| # | Service | Family | Client library | Live test | Status |
|---|---|---|---|---|---|
| 1 | MySQL | SQL | mysql_async | Homebrew MySQL | ✅ |
| 2 | MariaDB | SQL | mysql_async (MariaDB mode) | `mariadb:11` container | ✅ |
| 3 | PostgreSQL | SQL | tokio-postgres | Homebrew Postgres 16 | ✅ |
| 4 | SQLite | SQL | rusqlite (bundled) | local file | ✅ |
| 5 | SQL Server | SQL | tiberius (TDS, pure Rust, rustls) | `mssql/server:2022` container | ✅ |
| 6 | Db2 | SQL | ODBC (`odbc-api`) + IBM Data Server Driver | `icr.io/db2_community/db2` | ⏳ |
| 7 | Oracle | SQL | `oracle` (ODPI-C) + Oracle Instant Client | `gvenzl/oracle-free` | ⏳ |
| 8 | Kingbase | SQL | PostgreSQL wire protocol | none public (no free image) | 🟡 registered, untested |
| 9 | ClickHouse | SQL | HTTP interface | `clickhouse/clickhouse-server` | ✅ |
| 10 | JDBC | SQL | bundled Java bridge (needs a JRE) | via H2/any JDBC driver | ⏳ |
| 11 | SSH | Shell + SFTP | russh / russh-sftp | `linuxserver/openssh-server` | ✅ terminal, SFTP, commands, all auth methods but GSSAPI |
| 12 | Docker | Containers | bollard | Docker Desktop | ✅ |
| 13 | Redis | Key-value | redis | `redis:7` container | ✅ |
| 14 | Elasticsearch | Search | REST (reqwest) | `elasticsearch:8` | ✅ |
| 15 | MongoDB | Documents | mongodb | `mongo:7` | ✅ |
| 16 | S3 (AWS, MinIO, R2…) | Object storage | S3 REST (SigV4) | `chrislusf/seaweedfs` | ✅ + 13 provider presets |
| 17 | FTP / FTPS | Files | suppaftp | `delfer/alpine-ftp-server` | ✅ FTP, explicit/implicit FTPS |
| 18 | Kafka | Messaging | rskafka (pure Rust) | `apache/kafka` | ⏳ |
| 19 | RabbitMQ | Messaging | Management HTTP API | `rabbitmq:3-management` | ✅ |
| 20 | Cassandra | Wide-column (CQL) | scylla driver | `cassandra:5` | ⏳ |
| 21 | GaussDB / openGauss | SQL | PostgreSQL wire protocol | `opengauss/opengauss` | 🟡 registered, untested |

## Network paths (every TCP connector)
| Feature | Status |
|---|---|
| Direct | ✅ |
| SSH tunnel (password, private key, agent; host key pinning) | ✅ password + key tested · agent implemented, untested |
| Azure Blob Storage (key, SAS, connection string, Entra ID, anonymous, Azurite) | ✅ tested against Azurite |
| SOCKS5 proxy (with optional auth) | ✅ |
| HTTP CONNECT proxy (with optional auth) | ✅ |

## Honest limits
- **Oracle** and **Db2** need the vendor's free client library on the machine (Oracle Instant Client, IBM Data Server Driver). OpenVClient detects whether it's missing and says so; it never fakes the connection.
- **JDBC** runs drivers through a small Java bridge, so a Java runtime is required.
- **Kingbase** and **GaussDB** use the PostgreSQL protocol. Features only the PostgreSQL catalog provides are marked "PostgreSQL-compatible" until they're verified on the real engine.
