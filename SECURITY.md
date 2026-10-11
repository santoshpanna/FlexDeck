# Security policy

flexdeck runs actions on your computer, so we take reports seriously.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through GitHub:
[**Report a vulnerability**](https://github.com/santoshpanna/FlexDeck/security/advisories/new).

Include what you found, how to reproduce it, and what an attacker could do with
it. You will get a reply as soon as possible, and credit in the fix's release
notes if you want it.

## Supported versions

Security fixes go into the latest release of the desktop app and the Android app.

## How flexdeck is secured

[docs/security.md](docs/security.md) covers the design: TLS with certificate
pinning, per-channel credentials, and a cloud relay that holds no accounts and
nothing that can act on a desktop.
