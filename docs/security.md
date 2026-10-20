# Security

> Part of the [flexdeck docs](README.md). Found a vulnerability? Please report it
> privately — see [SECURITY.md](../SECURITY.md).

## Transport

The agent serves **HTTPS**, using the self-signed identity it already had for
pairing. This is not decoration: every API call carries `Authorization: Bearer
<token>` in a header, and over plain HTTP that token was readable by anything on
the network — which would have undone the authentication below entirely.

Clients pin the certificate by fingerprint. A browser will warn once about the
self-signed certificate; the Android client records the fingerprint on first
contact and refuses a *changed* one, which is what makes the pin worth having.

## Credentials and channels

Every `/api` route requires a credential, except the pairing redeem routes —
redeeming a code is *how* a client obtains one.

**Where a credential came in decides what it may do.** The agent tags every
request with its *channel* — a typed request extension set by the listener or
the tunnel forwarder, never a header — and each credential is honoured on one
channel only:

| Credential | Accepted on | May |
| --- | --- | --- |
| tablet (PIN/QR pairing) | LAN HTTPS + `/ws` | read the deck, press buttons |
| browser on this PC (first connect **approved in the desktop window**) | `127.0.0.1:<port+1>` only | configure everything |
| browser elsewhere (code pairing) | the cloud relay only | configure; plugin installs, tablet pairing windows and new program launches need **approval in the desktop window** |
| coding agent (Setup → Coding agents) | `127.0.0.1:<port+1>` `/mcp` only | edit the deck and automations; new program launches need **approval in the desktop window** |
| admin token | LAN + loopback, **never the relay** | everything (CLI) |

The cloud keeps no accounts and holds nothing that can act on a desktop: it
forwards a paired browser's own token, and the agent decides. A desktop proves
its identity to the cloud by **signing a fresh challenge** with its certificate
key, so knowing a desktop's fingerprint is not enough to take over its tunnel.
The loopback listener answers only `Host: 127.0.0.1/localhost` (DNS rebinding)
and only the portal's own origin (CORS, including Chrome's private-network
preflight). Safari may refuse an HTTPS page calling `http://127.0.0.1`; pairing
with a code works there instead. Opening a pairing window requires
the **admin** credential specifically, because its response contains the PIN and
the one-time secret: a paired tablet must not be able to mint windows of its own.
The same goes for anything that changes what code runs: rewriting the layout
(`PUT /api/config`, undo, redo), revoking devices, every plugin write, and the
web-access toggle. A device token can read the deck and press buttons — nothing
more — because a layout write could bind the launcher to any program.

**A program launch asks the person at the desktop.** A deck or automation write
from a browser elsewhere or from a coding agent that adds a launcher action, or
changes one's arguments, waits for a yes in the desktop window; no answer, or no
window to answer in, is a no. A step that presses keys or types (`core`
`hotkey` and `text`) in an automation, or in a key flow started by an event or a
slider rather than a press, counts as a launch too, because it runs with no one
at the keys. A launch whose program or arguments come from a variable or a
`{{template}}` cannot be approved from a line of text and is refused outright.
Undo and redo by those same callers are refused when the step would bring a
launch back. A browser on this PC and the admin
token are the person at the desktop and are not asked.

**A browser on this PC is let in at the desktop.** Every
`/api/local/connect` waits for a yes in the desktop window, so a local program
that forges the portal's `Origin` gets a prompt nobody asked for, not a token.
The headless `flexdeck` agent has no window to ask in: connect a browser there
with the admin token (`--show-admin-token`).

**Coding agent tokens** are revocable device rows of their own kind, honoured
only on `/mcp` behind the loopback guard; every other route refuses them.
They are stored under a hash a build without agent support never computes, so
rolling back to an older release does not turn one into a tablet token on the
LAN. "Remove all devices", in the desktop window or the editor, leaves them;
revoke them in Setup → Coding agents.

**Config writes are compare-and-swap.** `PUT /api/config` takes the revision it
was read at, from `If-Match` or the body's `rev`, and answers `409` with the
current revision if the deck has moved on; `PUT /api/automations` does the same
with its own `rev`. Every MCP write carries the revision the agent last read.

## Why every route is checked

This was not always true. An early probe of a *running* agent found the whole API
unauthenticated, which made the entire pairing ceremony bypassable in two
requests:

```
pairing/begin  -> PIN=955345  secret=9679ebf614bd...
pairing/redeem -> ok=True  tokenLength=64
PUT config     -> SUCCEEDED with no credentials
```

The lock was good; the key was hanging on the outside. Every unit test started
from a client that was already trusted, so none of them could see it.
