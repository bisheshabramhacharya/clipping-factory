# Security

## Reporting a vulnerability

Please do not open a public issue for a suspected vulnerability or exposed credential.

Use GitHub's private vulnerability reporting for this repository. Include the affected version, reproduction steps, expected impact, and any suggested mitigation.

## Local security model

Clipping Factory is designed to run on `127.0.0.1`. The studio has no accounts or authentication because it is intended for one user on one machine.

Do not set `CF_BIND_ALL=1` on an untrusted or public network. That option exposes project controls and local media endpoints to other devices that can reach the port.

Optional provider keys are stored in `~/.clipping-factory/settings.json` with user-only permissions on Unix systems. Never commit that file or copy its contents into an issue.
