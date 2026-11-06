## Summary

<!-- What does this change and why? Link issues with "Fixes #123". -->

## Type of change

- [ ] Bug fix
- [ ] New connector
- [ ] Connector improvement
- [ ] UI / UX
- [ ] Docs / CI / packaging
- [ ] Refactor (no behaviour change)

## How it was tested

<!-- Name the live servers and versions you tested against, the OS(es), and any test-case IDs from docs/TEST_CASES.md. -->

- [ ] `cargo fmt --all`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace` (live tests ran against: ...)
- [ ] Manual cases: <!-- e.g. CONN-008, SQL-005 -->

## Screenshots (UI changes)

<!-- Before / after, dark and light theme. The headless ui_walkthrough PNGs are fine. -->

## Checklist

- [ ] The UI only offers what the connector really supports (capability flags set honestly).
- [ ] Colours come from `theme::tokens(ui)`, icons from `egui_material_icons`; no raw hex, no emoji icons.
- [ ] No secrets in logs, errors or `workspace.json`.
- [ ] README / ROADMAP / docs/TEST_CASES.md updated where relevant.
