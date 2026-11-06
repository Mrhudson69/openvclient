# Prompt for building OpenVClient on Linux with Claude Code

Clone the repository on the Linux machine, open Claude Code in the clone, and
paste everything inside the block below.

```text
You are on a Linux desktop machine in a clone of https://github.com/Mrhudson69/openvclient
(OpenVClient: a Rust + egui desktop app for databases and infrastructure). Read CLAUDE.md,
README.md, CONTRIBUTING.md, docs/RELEASE_PLAN.md and docs/QA_REPORT.md first and follow them
(UI work uses the ui-ux-pro-max design rules in design-system/openvclient/MASTER.md: colours from
theme::tokens(ui), icons from egui_material_icons, no raw hex, no emoji icons).

Goal: the same app, fully working and packaged on Linux, installing like a native app (application
menu entry, icon, uninstall). Keep macOS and Windows working: put Linux-only code behind
#[cfg(target_os = "linux")] or #[cfg(unix)] as appropriate, and keep
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` clean.

1. Identify the distro, version, desktop (GNOME/KDE/other) and display server (Wayland or X11),
   and tell me. Check prerequisites and install only what is missing, telling me first:
   - Rust stable via rustup
   - build deps (see README "Linux"): build-essential/gcc, pkg-config, OpenSSL headers,
     xkbcommon, wayland, xcb, X11, GL headers
   - packaging tools: dpkg-deb (Debian/Ubuntu) or rpm-build (Fedora/openSUSE), python3-pil or
     ImageMagick (icon sizes), desktop-file-utils, appimagetool (pinned release from
     github.com/AppImage/appimagetool) and libfuse2 for running AppImages
   - a Secret Service provider for the keyring (gnome-keyring or KWallet) and xdg-desktop-portal
   - optional for live connector tests: Docker (see CLAUDE.md for the test containers)

2. Build and test:
   cargo build --workspace
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all -- --check
   Fix every error and warning. Typical Linux issues: missing system libraries, Wayland vs X11
   behaviour (window icon, decorations, scaling), the keyring when no Secret Service is running
   (the app must explain this clearly, never fall back to storing secrets in plain text), file
   dialogs through the portal, xdg-open for folders/links, and fonts/HiDPI scaling.

3. Run the app (cargo run --release -p openvclient-app) under the session you have (and the other
   display server too if you can, e.g. GDK_BACKEND/WAYLAND_DISPLAY unset for X11) and check:
   - window opens with the OpenVClient icon, sensible minimum size, Settings and New connection
     dialogs fit without clipping, light and dark themes are readable
   - saved passwords go to the Secret Service (check with seahorse or `secret-tool search service
     dev.openvclient.client`), and workspace history is encrypted at rest
     (crates/openvclient-app/src/vault.rs)
   - SSH: agent auth through SSH_AUTH_SOCK, key files, the host-key trust prompt for a new
     server, ~/.ssh/known_hosts checking, and the terminal tab (typing, arrows, Ctrl+C, paste,
     resize)
   - logs folder opens from Settings; import/export/backup file pickers work
   - the headless UI walkthrough:
     OPENVCLIENT_CONFIG_DIR=<copy of a workspace dir> OPENVCLIENT_SHOTS=<out dir> \
       cargo test -p openvclient-app ui_walkthrough -- --ignored --nocapture
     then look at the PNGs for layout problems.

4. Package and install:
   ./scripts/bundle-linux.sh
   It builds dist/openvclient-<version>-linux-<arch>.tar.gz (with install.sh), plus .deb, .rpm
   and AppImage when the tools are present. Install the native package for this distro
   (sudo apt install ./dist/openvclient_*.deb or sudo dnf install ./dist/openvclient-*.rpm) and
   check: OpenVClient appears in the application menu with its icon, launches from there and
   from a terminal (`openvclient`), the window is grouped under the right icon in the dock
   (StartupWMClass), it shows correctly in GNOME Software/KDE Discover (AppStream metadata), and
   uninstalling removes the menu entry and icon. Also try the AppImage and the tar.gz install.sh
   (per-user and --system, then --uninstall). Fix anything that looks unpolished.

5. Report back with: what you changed and why, the exact commands and their results, what you
   tested by hand (distro, desktop, Wayland/X11), and anything still not working or untested.
   Commit on a new branch named linux-compat and open a pull request against main (do not push
   to main directly); make sure CI on the PR is green. Do not add personal email addresses or
   secrets to the repository.
```
