# Security Policy

## Supported Versions

Security fixes are applied to the latest released version on `main`.

Older releases are not guaranteed to receive backported fixes.

## Reporting a Vulnerability

Please do not report sensitive security issues in public GitHub issues.

Preferred reporting path: open a private advisory at
<https://github.com/nicx17/mimick/security/advisories/new>.

This reaches the maintainer privately, creates an audit trail, and lets us
coordinate a fix and CVE before public disclosure.

### Disclosure timeline

- **Acknowledgement** within 7 days of the report.
- **Initial assessment** within 30 days.
- **Coordinated public disclosure** targeted within 90 days, sooner if a fix
  is available and the vulnerability is being actively exploited.

When reporting a vulnerability, include:

- the Mimick version
- how it was installed (`Flatpak`, local build, etc.)
- the affected operating system and desktop environment
- reproduction steps or a proof of concept
- whether the issue can expose files, API keys, or remote account access

## Security Practices in This Project

Mimick already uses several controls intended to reduce risk:

- release assets include checksums
- the API key is stored in the desktop keyring instead of plain-text config
- Flatpak builds use selected-folder access instead of broad home-directory access
- [Semgrep](https://semgrep.dev) static analysis runs on every push and PR
- [Dependabot](https://docs.github.com/en/code-security/dependabot) keeps Rust and GitHub Actions dependencies patched
- CI enforces formatting, linting, tests, and `cargo audit` dependency scanning


