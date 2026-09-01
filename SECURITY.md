# Security

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability or exposed credential.

Use GitHub's private vulnerability reporting for this repository. Include the affected version, reproduction steps, expected impact, and any suggested mitigation.

## Local security model

Clipping Factory is designed to run on `127.0.0.1`. The studio has no accounts or authentication because it is intended for one user on one machine.

The server always binds to `127.0.0.1`; there is no supported setting to expose project controls or local media endpoints on other interfaces. Do not use a reverse proxy or port forward to expose the studio without adding authentication and a deliberate network security model.

Optional provider keys are stored in `~/.clipping-factory/settings.json` with user-only permissions on Unix systems. Never commit that file or copy its contents into an issue.
