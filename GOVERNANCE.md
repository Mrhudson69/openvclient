# Governance

This document describes the roles in the OpenVClient project, what each role can do, and how people move between roles.

## Roles

| Role | GitHub access | Team | What they handle |
|---|---|---|---|
| **Contributor** | None (works from a fork) | none | Opens issues and pull requests. Anyone can be a contributor. |
| **Triager** | Triage | `@openvclient/triage` | Labels, assigns and closes issues and PRs, marks duplicates, asks for reproduction steps. Cannot push code. |
| **Docs maintainer** | Write | `@openvclient/docs` | Reviews and approves changes to `docs/`, `README.md`, `ROADMAP.md`, `CHANGELOG.md` and `CONTRIBUTING.md`. |
| **Connector maintainer** | Write | `@openvclient/connectors` | Reviews and approves changes to connector crates (`crates/openvclient-{docker,http,mongo,mysql,postgres,redis,sqlite,sqlserver}`). Keeps connector status in the README and ROADMAP honest. |
| **Core maintainer** | Write | `@openvclient/core` | Reviews and approves changes to the app, UI, core abstractions and storage (`openvclient-app`, `openvclient-core`, `openvclient-storage`, `design-system`, `assets`). |
| **Maintainer** | Maintain | `@openvclient/maintainers` | Owns everything not assigned to another team, plus all security-sensitive areas: `openvclient-net` (SSH, TLS, proxies), `.github/` (CI and workflows), `installer/`, `scripts/`, dependencies (`Cargo.toml`, `Cargo.lock`), releases, and project policy. Handles security reports. |
| **Owner** | Admin | — | Repository settings, rulesets, secrets, signing keys, org membership and role changes. Currently: Rohan Sirohi (@Mrhudson69). |

Code ownership is enforced through [`CODEOWNERS`](.github/CODEOWNERS). A PR is automatically sent to the owning team for review.

## How changes get merged

- `main` is protected. Every change goes through a pull request; nobody pushes to `main` directly.
- A PR needs **1 approving review** from someone other than the last person who pushed to it, all CI checks passing, and all review conversations resolved.
- When a PR touches an owned area, the approval must come from that area's team (or a maintainer).
- Only squash and rebase merges are allowed, so history stays linear.
- Changes to security-sensitive areas (listed under Maintainer above) need a maintainer's approval.

## Least privilege

- Access is granted per team, never to individuals directly, and only at the level the role needs.
- Write access lets you push branches to this repository, but not to `main`.
- Only the Owner can change repository settings, rulesets, secrets, or release signing.
- Repository secrets (signing certificates, release tokens) are only exposed to workflows on `main` and tags, never to pull requests from forks.
- Inactive members (no activity for 6 months) are moved back to Contributor. They are welcome to return.

## Becoming a maintainer

1. **Triager**: offered after steady, helpful participation in issues and discussions.
2. **Docs, connector or core maintainer**: offered after several merged, quality PRs in that area and good review comments on other people's PRs.
3. **Maintainer**: offered to area maintainers who have shown sound judgment across the project over time, including on security-sensitive changes.

Nominations come from an existing maintainer and are confirmed by the Owner. Anyone can also ask to be considered by opening an issue or emailing the maintainers.

## Security

Do not open public issues for vulnerabilities. Follow [SECURITY.md](SECURITY.md). Security reports are handled by maintainers only.

## Code of Conduct

Everyone in the project follows the [Code of Conduct](CODE_OF_CONDUCT.md). Maintainers enforce it, and the Owner makes final decisions on enforcement.
