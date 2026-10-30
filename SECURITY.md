# Security Policy

## Reporting a vulnerability

Report privately through GitHub Security Advisories: go to the **Security** tab of this repository and choose **Report a vulnerability**.

Please do not open a public issue for security problems. Include steps to reproduce, the affected version and your OS. Do not attach real chat data.

We aim to acknowledge reports within 7 days, and will keep you updated until the issue is fixed or ruled out.

## Scope

In scope:

- Anything that makes the app read a source the user has not allowed
- Any network call other than the update check described in PRIVACY.md and opening a share link the user clicks
- Ways to make the app install an update not signed with the project's updater key
- Correction text kept longer, or at a higher level, than the retention setting allows
- Redaction bypasses (secrets, emails, URLs, home paths surviving in stored text)
- Bugs in the SQLite store, source adapters or the Tauri shell that could expose or corrupt local data
- Vulnerable dependencies that affect the shipped app

Out of scope:

- Attacks that need an attacker who already has full access to your user account or disk (the database is local and not encrypted)
- Wrong correction counts or labels (use the "Wrong correction" issue form)
- Issues in the AI tools whose history the app reads

## Supported versions

| Version | Supported |
|---|---|
| Latest release | Yes |
| Older releases | No. Please update. |
