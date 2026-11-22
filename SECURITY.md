# Security policy

## Reporting a vulnerability

Please report security problems privately, not in a public issue:

1. Go to the repository's [Security tab](https://github.com/monishbl/tessera/security).
2. Choose **Report a vulnerability** ([direct link](https://github.com/monishbl/tessera/security/advisories/new)).
3. Describe what's affected, how to reproduce it, and what an attacker could do.

You'll get an answer in the advisory. Once a fix is out, the advisory is published with credit to you, unless you'd rather not be named.

Please don't include real passwords, API keys or personal data in a report; redact them.

## What's in scope

- The installer, `setup.sh`, the `/get` script and Software Update (`tools/`, `install.sh`, `uninstall.sh`)
- The root helper and its polkit rules, the login and lock screens
- Quick Share, LocalSend and the firewall rules Tessera opens and closes
- Credential storage for AI providers and online accounts
- tessera-ui and tessera-events (`native/`) and the tessera-glass plugin (`hypr/plugin/`)
- Signed ready-made builds and how they're checked ([docs/releases.md](docs/releases.md))

Problems in Hyprland, Quickshell, Slint or other upstream projects belong with them, but tell us too if Tessera makes them worse.

## Supported versions

Only the latest Stable (the `main` branch) and Beta (the `beta` branch) get fixes. Software Update and `tessera update` move you to the latest.
