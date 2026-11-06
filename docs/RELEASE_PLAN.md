# OpenVClient release plan

This plan covers how OpenVClient is built, signed, versioned and shipped on macOS, Windows and Linux. The automation lives in [`.github/workflows/release.yml`](../.github/workflows/release.yml). Packaging scripts live in [`scripts/`](../scripts).

## 1. Targets and artefacts

| Platform | Rust target(s) | Artefacts | Phase |
|---|---|---|---|
| macOS 11+ (Apple silicon + Intel) | `aarch64-apple-darwin`, `x86_64-apple-darwin` → `lipo` universal | `OpenVClient.app` in `OpenVClient-<v>-macos-universal.dmg` and `.zip` | 0.1 |
| Windows 10/11 x86_64 | `x86_64-pc-windows-msvc` | `OpenVClient-<v>-windows-x86_64.zip` (portable `.exe`) | 0.1 |
| Windows x86_64 installer | same | `OpenVClient-<v>-windows-x64-setup.exe` (Inno Setup 6.6+, `installer/windows/openvclient.iss`) | 0.1 |
| Windows 11 ARM64 | `aarch64-pc-windows-msvc` | zip + MSI | 0.3 |
| Linux x86_64 | `x86_64-unknown-linux-gnu` (glibc 2.35 baseline, built on Ubuntu 22.04) | `openvclient-<v>-linux-x86_64.tar.gz`, `openvclient_<v>_amd64.deb` | 0.1 |
| Linux x86_64 extra formats | same | AppImage, `.rpm` (cargo-generate-rpm) | 0.2 |
| Linux aarch64 | `aarch64-unknown-linux-gnu` (native `ubuntu-24.04-arm` runner) | tar.gz, deb, rpm, AppImage | 0.3 |
| Linux Flatpak | — | Flathub manifest (`dev.openvclient.OpenVClient`) | after 1.0 |
| All | — | `SHA256SUMS`, `SHA256SUMS.asc` (GPG) | 0.1 (sig: 0.2) |

Every release also gets the source tarball that GitHub produces automatically.

**Bundle identifiers**: `dev.openvclient.client` (macOS `CFBundleIdentifier`, the same as the keychain service name) and `dev.openvclient.OpenVClient` (Flatpak/AppStream).

## 2. Versioning

- The project uses **SemVer**. The single source of truth is `[workspace.package] version` in the root `Cargo.toml`. Every crate uses `version.workspace = true`.
- Tags are **`vX.Y.Z`**, with pre-releases like `v0.2.0-rc.1`. `release.yml` triggers on `v*` and **refuses to build** when the tag doesn't match the Cargo version.
- Before 1.0, a **minor** bump may break the `workspace.json` format or the connector traits, and a patch is for fixes only. After 1.0, `workspace.json` changes must be backward compatible (old files keep loading, and `serde(default)` is used for new fields).
- Connector crates aren't published to crates.io before 1.0. The app is the product.

## 3. Code signing and notarisation

All signing steps are **in `release.yml` already but only run when their secrets exist**, so forks and unsigned pre-releases still build.

### macOS: Developer ID + notarytool
| Secret | Purpose |
|---|---|
| `MACOS_CERTIFICATE` / `MACOS_CERTIFICATE_PASSWORD` | base64 `.p12` of the *Developer ID Application* certificate |
| `MACOS_SIGNING_IDENTITY` | e.g. `Developer ID Application: OpenVClient (TEAMID)` |
| `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD` | `xcrun notarytool submit --wait` credentials |

The steps are:
1. Import the certificate into a temporary keychain.
2. Run `codesign --force --options runtime --timestamp --sign "$IDENTITY" OpenVClient.app`. Hardened runtime is on. No entitlements are needed, because the app uses no JIT and the keychain works under the app's own identity.
3. Build the `.dmg`, sign it, submit it to `notarytool`, then run `xcrun stapler staple` on the dmg.
4. Without secrets, the build falls back to the **ad-hoc** signature (`codesign --sign -`), like `scripts/bundle-macos.sh` does. Users must right-click → Open on first launch.

**Keychain note:** the keychain ACL is tied to the code signature. Moving from ad-hoc to Developer ID makes macOS ask again once for each stored secret. Mention this in the release notes of the first signed build.

### Windows: Authenticode
The options, in order of preference for an OSS project:
1. **SignPath Foundation** (free for OSS): its GitHub Action signs the artefacts and produces an EV-equivalent reputation.
2. **Azure Trusted Signing** (paid per month, cheap): `azure/trusted-signing-action` with the secrets `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `TRUSTED_SIGNING_ACCOUNT` and `TRUSTED_SIGNING_PROFILE`.
3. A classic OV certificate in `WINDOWS_CERTIFICATE` (base64 `.pfx`) and `WINDOWS_CERTIFICATE_PASSWORD` with `signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256`. **This is what `release.yml` implements today.**

Sign `openvclient.exe` first, then the setup `.exe` (`scripts/bundle-windows.ps1` does both).

### Linux
- Packages aren't signed individually at first. Users verify **`SHA256SUMS` signed with the project GPG key** (`GPG_PRIVATE_KEY`, `GPG_PASSPHRASE`). The public key and its fingerprint are published in the README and on keys.openpgp.org.
- Later, an apt/rpm repository signed with the same key. The Flatpak is signed by Flathub.

## 4. Platform-specific notes

| Topic | macOS | Windows | Linux |
|---|---|---|---|
| Secrets backend (`keyring` 4) | Keychain (`apple-native-keyring-store`) | Credential Manager (`windows-native-keyring-store`) | Secret Service over D-Bus, pure Rust zbus (`zbus-secret-service-keyring-store`). **Needs** gnome-keyring, KWallet or KeePassXC at runtime. |
| SQL Server `windows`/`integrated` auth | not available (clear error) | **available**: tiberius `winauth` (SSPI) is enabled only under `cfg(windows)` | not available (clear error). Kerberos/GSSAPI isn't wired up. |
| SQL Server Browser (named instances) | `sql-browser-tokio` | `sql-browser-tokio` | `sql-browser-tokio` |
| TLS for PG/MySQL/Redis (`native-tls`) | Security.framework | SChannel | **OpenSSL** (`libssl3`/`openssl-libs` runtime dependency for deb/rpm). The AppImage bundles nothing and relies on the system libssl. |
| TLS for SQL Server, HTTP connectors | rustls | rustls | rustls |
| Renderer | eframe `glow` (OpenGL 3.2 core via CGL) | `glow` through WGL; when OpenGL 2+ is missing (Remote Desktop, VMs with the Basic Display Adapter, old drivers) the app retries with `wgpu` on Direct3D 12, which falls back to WARP software rendering. | `glow` via EGL/GLX, with X11 and Wayland both compiled in. |
| GPU fallback | — | Built in: `wgpu` (D3D12 / WARP), compiled into Windows builds only. If both renderers fail, a dialog explains why instead of exiting silently. | Mesa llvmpipe works out of the box (PKG-009). |
| wgpu | used only by `egui_kittest` in tests | renderer fallback (see above) and tests | same. Switching the app to `wgpu` (Metal/DX12/Vulkan) is an option once it's measured; it is not planned for 0.x. |
| File dialogs (`rfd` 0.17) | NSOpenPanel | IFileDialog | **xdg-desktop-portal** (no GTK needed at build time) |
| Console window | — | `windows_subsystem = "windows"` in release builds | — |
| Config dir | `~/Library/Application Support/dev.OpenVClient.OpenVClient` | `%APPDATA%\OpenVClient\OpenVClient\config` | `~/.config/openvclient` |
| External backup tools | `pg_dump`, `mysqldump` from Homebrew | On `PATH` or in the installers' default folders (`Program Files\PostgreSQL\<v>in`, `MySQL\MySQL Server <v>in`, `MariaDB <v>in`); not bundled. Started without a console window. | distribution packages |
| Docker | `/var/run/docker.sock` (Docker Desktop, Colima, OrbStack) | named pipe `//./pipe/docker_engine` | `/var/run/docker.sock` (the user is in the `docker` group) |

We **don't bundle** `pg_dump`, `mysqldump` or the Azure CLI. They are found at runtime, and the matching actions are hidden when a tool is missing. That keeps the "never pretend" rule.

## 5. Release checklist

**One week before**
- [ ] Open a "Release vX.Y.Z" issue from this checklist.
- [ ] Update dependencies with `cargo update` and review the Dependabot PRs. Run `cargo deny check`, or at least `cargo audit`.
- [ ] Update ROADMAP.md statuses. Only mark ✅ for connectors that passed their live test.
- [ ] Freeze features. Only fixes go into `main`.

**Release candidate**
- [ ] Bump `version` in the root `Cargo.toml`, then run `cargo check` so `Cargo.lock` updates.
- [ ] Write the `CHANGELOG.md` entry with Added / Changed / Fixed / Security sections and any keychain or workspace migration notes.
- [ ] Tag `vX.Y.Z-rc.1` and push. Confirm that `release.yml` produces every artefact in a draft release.
- [ ] Run all release-blocking (**R**) manual cases from [TEST_CASES.md](TEST_CASES.md) on each OS in the matrix below, and record the results in the issue.
- [ ] Run the headless walkthrough and attach the screenshots to the issue.

**Release**
- [ ] Tag `vX.Y.Z` on the verified commit and push.
- [ ] Check the draft release: every artefact is present, `SHA256SUMS` verifies, the macOS build is notarised (`spctl -a -vv`) and the Windows signature is valid (`Get-AuthenticodeSignature`).
- [ ] Paste the changelog into the release notes and include the checksum and verification instructions.
- [ ] Publish the release, and mark it as a pre-release before 1.0.
- [ ] Update the README screenshots and the download links, and announce the release.

**After the release**
- [ ] Watch issues for 72 hours, and ship a patch release for regressions.
- [ ] Bump `main` to the next `-dev` version (optional).

## 6. Auto-update strategy (later)

There is **no auto-update and no update check** before 1.0, which matches the no-telemetry promise.

1. **0.4**: an opt-in "Check for updates" button in Settings. It does one HTTPS GET against the GitHub Releases API, sends no identifiers, and only shows a notice.
2. **1.x**: opt-in in-app updates through signed manifests (for example `self_update` with an ed25519 signature check, or Sparkle on macOS and WinGet/MSI upgrade codes on Windows). Linux users update through their package manager, Flatpak or AppImageUpdate.
3. Distribution channels: Homebrew cask, WinGet, Scoop, Flathub and the AUR, once signing is in place.

## 7. Milestones

| Milestone | Scope | Exit criteria |
|---|---|---|
| **0.1 preview** | Current connectors (SQLite, Postgres, MySQL/MariaDB, SQL Server, ClickHouse, Redis, MongoDB, Elasticsearch, Docker) and network paths. Unsigned or ad-hoc artefacts: macOS universal dmg, Windows zip, Linux tar.gz + deb. | CI green on 3 OSes. Live tests pass in Linux CI. Manual **R** cases pass on macOS 14+, Windows 11 and Ubuntu 24.04. |
| **0.2** | Developer ID and notarisation, Authenticode, GPG-signed checksums. MSI, AppImage, rpm. Real screenshots. RabbitMQ and S3 if their live tests pass. | Signed artefacts verify. No unsigned binaries. |
| **0.3** | ARM64 for Windows and Linux. SSH shell and SFTP, FTP. Live network-path tests in CI. | The arm64 smoke tests (PKG-005) pass. NET-* are automated. |
| **0.4** | Oracle and Db2 (vendor client detection), Kafka, Cassandra. Opt-in update check. | Each new connector is ✅ in ROADMAP. |
| **0.5–0.9** | JDBC bridge. Accessibility audit. Performance budget (cold start < 1 s, 1M-row scroll at 60 fps). Workspace format freeze. | No open P1 bugs. Docs complete. |
| **1.0** | Stable `workspace.json` format, stable trait API for out-of-tree connectors, signed packages on every channel, Flatpak. | Two release candidates without blocking regressions. |

## 8. Test matrix

✅ means automated in CI. **M** means a manual case in the release checklist. — means not applicable or not tested.

| Area | Linux x86_64 (CI) | macOS arm64 (CI) | macOS x86_64 | Windows x86_64 (CI) | Windows arm64 | Linux arm64 |
|---|---|---|---|---|---|---|
| `cargo fmt` / clippy | ✅ | — | — | ✅ clippy | — | — |
| Build + unit tests | ✅ | ✅ | M (Rosetta smoke) | ✅ | M | M |
| SQLite end-to-end | ✅ | ✅ | — | ✅ | — | — |
| Postgres / MySQL / Redis live | ✅ (service containers) | — (skips) | — | — (skips) | — | — |
| SQL Server, Mongo, ClickHouse, ES live | ✅ (service containers) | — | — | — | — | — |
| RabbitMQ / S3 live (in development) | ✅ (containers) | — | — | — | — | — |
| Docker live | ✅ (runner daemon; container checks skip) | — | — | — | — | — |
| Network paths (SSH, SOCKS5, HTTP) | M (planned for CI in 0.3) | M | — | M | — | — |
| SQL Server `sql` + Entra auth | ✅ sql / M entra | M | — | M | — | — |
| SQL Server `windows` / `integrated` | error path ✅ | error path ✅ | — | **M (only here)** | M | — |
| Keychain backend | M (gnome-keyring + KWallet) | M | M | M | M | M |
| Headless UI walkthrough | M (needs seeded workspace) | M | — | M | — | — |
| Packaging smoke PKG-* | M | M | M | M | M | M |
| HiDPI / Wayland / X11 | M (both) | M (Retina) | — | M (150%) | — | M |
