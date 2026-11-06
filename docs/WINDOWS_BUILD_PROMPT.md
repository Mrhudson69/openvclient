# Prompt for building OpenVClient on Windows with Claude Code

Clone the repository on the Windows machine, open Claude Code in the clone, and
paste everything inside the block below.

```text
You are on a Windows 10/11 machine in a clone of https://github.com/Mrhudson69/openvclient
(OpenVClient: a Rust + egui desktop app for databases and infrastructure). Read CLAUDE.md,
README.md, CONTRIBUTING.md, docs/RELEASE_PLAN.md and docs/QA_REPORT.md first and follow them
(UI work uses the ui-ux-pro-max design rules in design-system/openvclient/MASTER.md: colours from
theme::tokens(ui), icons from egui_material_icons, no raw hex, no emoji icons).

Goal: the same app, fully working and packaged on Windows, with a professional installer.
Keep macOS and Linux working: put Windows-only code behind #[cfg(windows)], never break the other
platforms, and keep `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo fmt --all -- --check` clean.

1. Prerequisites (check each, install only what is missing, tell me before installing):
   - Rust stable via rustup (x86_64-pc-windows-msvc toolchain)
   - Visual Studio 2022 Build Tools with "Desktop development with C++" (MSVC + Windows 10/11 SDK;
     needed for C dependencies and rc.exe, which embeds the app icon)
   - Git, PowerShell 7 (pwsh)
   - Inno Setup 6 for the installer: winget install JRSoftware.InnoSetup
   - Optional for live connector tests: Docker Desktop (see CLAUDE.md for the test containers)

2. Build and test:
   cargo build --workspace
   cargo test --workspace
   cargo clippy --workspace --all-targets -- -D warnings
   cargo fmt --all -- --check
   Fix every error and warning. Typical Windows issues to look for: path handling (\ vs /,
   %APPDATA% config dir), opening files/folders (explorer), process spawning (.exe suffixes,
   pg_dump/mysqldump discovery on PATH), line endings in tests, file permissions code that is
   Unix-only, terminal key handling, and anything behind #[cfg(unix)] that needs a Windows twin.

3. Run the app (cargo run --release -p openvclient-app) and check by hand:
   - window opens with the OpenVClient icon in the title bar and taskbar, no console window
   - minimum window size, Settings and New connection dialogs fit without clipping
   - saved passwords go to Windows Credential Manager (service "dev.openvclient.client"), and
     workspace history is encrypted at rest (see crates/openvclient-app/src/vault.rs)
   - SQL Server "windows" (NTLM user/password) and "integrated" (current Windows login) auth,
     which only exist in the Windows build (tiberius winauth feature): test against a reachable
     SQL Server if one is available; otherwise report that they are untested
   - SSH agent auth through the Windows OpenSSH agent pipe and Pageant
   - SSH terminal tab: typing, Enter, arrows, Ctrl+C, paste, resize
   - the SSH host-key trust prompt appears for a new server
   - logs folder opens from Settings; file pickers for import/export/backup work
   - the headless UI walkthrough:
     $env:OPENVCLIENT_CONFIG_DIR="<copy of a workspace dir>"; $env:OPENVCLIENT_SHOTS="<out dir>"
     cargo test -p openvclient-app ui_walkthrough -- --ignored --nocapture
     then look at the PNGs for layout problems.

4. Build the installer:
   .\scripts\bundle-windows.ps1
   This must produce dist\OpenVClient-<version>-windows-x64-setup.exe (Inno Setup wizard from
   installer\windows\openvclient.iss) and a portable zip. Run the setup and walk through every page:
   Welcome -> License -> install for me / all users -> folder -> Start menu -> desktop shortcut
   option -> Ready -> Finish with "Launch OpenVClient". Confirm the app is listed in
   Settings > Apps with its icon, version and publisher, that upgrading over an existing install
   works, and that uninstalling removes the program and asks before deleting user data.
   Fix anything that looks unpolished (wizard images, texts, icon, shortcuts).

5. Report back with: what you changed and why, the exact commands and their results, what you
   tested by hand, and anything still not working or untested. Commit on a new branch named
   windows-compat and open a pull request against main (do not push to main directly); make sure
   CI on the PR is green. Do not add personal email addresses or secrets to the repository.
```
