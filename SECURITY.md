# Security Policy

OpenVClient handles database credentials and connects to production systems, so we take security reports seriously.

## Supported versions

| Version | Supported |
|---|---|
| Latest release (and `main`) | Yes |
| Older pre-1.0 releases | No. Please upgrade. |

After 1.0, the latest minor release gets security fixes.

## Reporting a vulnerability

**Please do not open a public issue, discussion or pull request for a security problem.**

Report it privately through **GitHub Security Advisories**:

1. Go to the repository's **Security** tab.
2. Click **Report a vulnerability** to open a private advisory draft. Only the maintainers can see it.
3. Include:
   - the affected version or commit, and your OS and architecture
   - the connector or feature involved (for example SSH tunnel, keychain, SQL Server Entra auth)
   - reproduction steps or a proof of concept
   - the impact as you understand it (for example credential disclosure, host-key bypass, code execution)

Please remove real passwords, tokens and host names from logs before you attach them. OpenVClient's logs are designed to contain no secrets. If you find one in a log, that is itself a vulnerability, so please report it.

### What to expect

| Step | Target |
|---|---|
| Acknowledgement | within 3 working days |
| Initial assessment and severity | within 10 working days |
| Fix or mitigation for high or critical issues | as fast as practical, normally within 30 days |
| Public advisory and CVE (through GitHub) | when the fix is released, crediting you unless you prefer not to be named |

We will keep you informed and may ask for more details. Please give us a reasonable chance to fix the issue before you disclose it publicly.

## In scope

- Secrets leaking out of the OS keychain: into `workspace.json`, logs, error messages, crash output or the clipboard without user action.
- Bypassing SSH host-key pinning (trust on first use) or proxy authentication.
- TLS verification that is weaker than the option the user selected.
- SQL or command injection through identifiers, filters or the generated DDL/DML that the app builds on the user's behalf.
- Destructive actions running without the documented confirmation, including the production-environment safety rail and read-only profiles.
- Problems in the release artefacts, such as signing, checksums and update channels once they exist.

## Out of scope

- Statements that a user deliberately types and runs in the SQL editor or console.
- Vulnerabilities in the database servers themselves. Report those to the vendor.
- Problems that need an already compromised local account or OS keychain.

## Security model (summary)

- Secrets live only in the OS keychain (service `dev.openvclient.client`).
- SSH host keys are pinned by SHA-256 fingerprint on first use. A changed key is refused.
- There is no telemetry, analytics or auto-update. The only network traffic goes to the servers you configure, plus Microsoft Entra ID endpoints when you use Entra auth.
