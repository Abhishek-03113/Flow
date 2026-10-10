# Security policy

Flow forwards keyboard and mouse input between computers, so security reports are taken seriously.

## Reporting a vulnerability

Please do **not** open a public issue. Use GitHub's private reporting:
**Security tab > Report a vulnerability** on this repository. Include affected version/OS, reproduction steps and
impact. You can expect an acknowledgement within a few days.

## Scope and design

Devices pair once and then authenticate each other with Ed25519 identity keys; sessions run over a Noise-encrypted
channel. See [`docs/product/vision.md`](docs/product/vision.md) (security model), [`docs/architecture/channels.md`](docs/architecture/channels.md)
and `daemon/src/security/` for details. Reports about pairing bypass, key handling, input leaking to an unintended
device, or the local control API (loopback WebSocket with a token) are especially welcome.
