# Editing the deck from a coding agent (MCP)

The FlexDeck desktop app serves a [Model Context Protocol](https://modelcontextprotocol.io) endpoint, so an AI coding agent on the same computer (Claude Code, Cursor, VS Code) can read and change your deck: add keys, move them, give them actions and looks, and edit automations.

It only answers on this computer, at `http://127.0.0.1:<port+1>/mcp` (the installed app uses `http://127.0.0.1:23540/mcp`). Tablets, other computers and the cloud relay cannot reach it.

## Connect an agent

1. Open the FlexDeck app, go to **Setup → Coding agents**, name the token (the agent's name is a good choice) and press **Create token**.
2. Copy the snippet for your tool. The token is shown once; don't commit it.

Claude Code:

```
claude mcp add --transport http --scope user flexdeck http://127.0.0.1:23540/mcp --header "Authorization: Bearer <token>"
```

Cursor (`~/.cursor/mcp.json`):

```json
{ "mcpServers": { "flexdeck": { "url": "http://127.0.0.1:23540/mcp", "headers": { "Authorization": "Bearer <token>" } } } }
```

VS Code (`.vscode/mcp.json` or the user `mcp.json`):

```json
{ "servers": { "flexdeck": { "type": "http", "url": "http://127.0.0.1:23540/mcp", "headers": { "Authorization": "Bearer <token>" } } } }
```

To cut an agent off, revoke its token in the same Setup section. A token works only on `/mcp`, only from this computer, and does nothing else: it cannot read the rest of the API, pair devices, install plugins or see secrets.

## What an agent can do, and what asks you first

An agent can change everything on the deck and in automations, and a deck key can run a program on this computer (the **launcher** plugin), on a press or on a schedule. So whenever an agent's change **adds** a program launch the deck did not already have, the FlexDeck window asks you first. A step that presses keys or types text (`core` `hotkey` and `text`) asks too when it runs with nobody at the keyboard: in an automation, or in a key flow started by an event or a slider instead of a press. A launch whose program or arguments come from a variable, a slider, or a `{{template}}` is refused; add that one at the desktop. If the app's window is not running to ask in, the change is refused. Undo and redo through an agent are refused when the step would bring a launch back; do those at the desktop.

Secret arguments (API keys, tokens a data source needs) are never set through MCP. Set them in the portal.

## The revision rule

The deck has a revision number, `rev`, that goes up with every saved change from anywhere: the portal, a tablet, an agent. Every write tool takes the `rev` you last read and returns the new one. If someone else changed the deck in between, the write is refused with the current `rev`: read again (`deck_overview` or `get_widget`), redo your change against what is there now, and retry. Automations have their own counter, `automationsRev`, with the same rule.

## Tools

| Tool | What it does |
| --- | --- |
| `deck_overview` | Revision, and every profile's grid and pages with each widget's id, place, size, label and press action. Start here. |
| `get_deck` | Part of the deck's JSON by JSON Pointer, for `patch_deck`. |
| `get_widget` | One widget as stored, its page, and why it cannot work right now if a plugin is missing. |
| `get_guide` | This page. |
| `list_actions` | The actions plugins offer and their arguments. |
| `list_choices` | The values an action's arguments can take right now: scenes, devices, players. |
| `list_variables` | Variables a key can show or react to, with current values. Search with `query` (words in the name or description) and `prefix`; 50 shown by default, `limit` up to 500, `matched` gives the total. |
| `preview_widget` | How a widget would look now, without saving it. |
| `upsert_widget` | Add a widget, or replace the one with the same id. |
| `move_widget` | Move or resize a widget, also onto another page. |
| `remove_widget` | Remove a widget. |
| `add_page`, `rename_page`, `remove_page` | Pages. A profile keeps at least one. |
| `patch_deck` | RFC 6902 JSON Patch on the whole deck, for anything else. |
| `undo`, `redo` | Step through saved changes. |
| `get_automations`, `put_automations` | Read and replace the automations document. |
| `dry_run_automation` | What an automation would do, without doing it. |

Every write is checked like an edit in the portal: widgets must fit the grid and not overlap, ids must be unique, and each widget must be one the deck can run. A refused write changes nothing.

## How a deck is built

```
deck
└── profiles[]          { id, name, grid: { cols, rows, ... }, pages[] }
    └── pages[]         { id, name, widgets[] }
        └── widgets[]   { id, type, col, row, w, h, props, flows, states, ... }
```

- `type` is `WIDGET_TYPE_BUTTON` (a key, 1×1 up to 4×4 cells) or `WIDGET_TYPE_SLIDER` (up to 8 cells either way). `col`/`row` count from 0 at the top left; `w`/`h` are cells.
- `props` is the plain look: `label`, `icon`, `bgColor`/`fgColor` (`"#1e40af"`), `labelTemplate`/`iconTemplate` (text with `{{variable.name}}` in it).
- `flows` is what the key does. Each flow has a `trigger` and `blocks` that run in order. Triggers: `{"gesture": "GESTURE_SHORT_PRESS"}` (also `GESTURE_LONG_PRESS`, `GESTURE_DOUBLE_TAP`, `GESTURE_HOLD_START`, `GESTURE_HOLD_END`), `{"event": {...}}` for a plugin event, `{"value": {}}` for a slider settling on a value.
- Blocks are `{"block": "doAction", "plugin": "...", "action": "...", "args": {...}}`, plus `if`, `repeat`, `wait` and `setVariable`. Use `list_actions` for real plugin and action names and `list_choices` for argument values.
- Each argument is a value object: `{"text": "Live"}`, `{"number": 50}`, `{"bool": true}`, `{"variable": "media.current.title"}` (read when the key runs), or `{"input": "value"}` (a slider's position).
- `states` changes the look from moment to moment: `looks` is a list of named looks (`{"id": "on", "bgColor": "#16a34a"}`), and `variable` picks one from a variable's value: `{"variable": "name", "cases": [{"value": "true", "state": "on"}], "otherwise": "off"}`. A case can also use `"when"` with a condition such as `"media.spotify.position > 60"`.

A complete key that plays or pauses Spotify, shows the track, and lights up while it plays:

```json
{
  "type": "WIDGET_TYPE_BUTTON",
  "w": 1, "h": 1,
  "props": { "label": "Spotify", "labelTemplate": "{{media.spotify.title}}" },
  "flows": [
    {
      "trigger": { "gesture": "GESTURE_SHORT_PRESS" },
      "blocks": [
        { "block": "doAction", "plugin": "media", "action": "session_play_pause",
          "args": { "app": { "text": "spotify" } } }
      ]
    }
  ],
  "states": {
    "looks": [ { "id": "off" }, { "id": "on" } ],
    "variable": { "variable": "media.spotify.playing", "cases": [ { "value": "true", "state": "on" } ], "otherwise": "off" }
  }
}
```

You don't need to write the `states` part for an action that can tell when it is active: `upsert_widget` adds it unless you pass `"lit": "none"`. Leave out `id` to add a widget (it gets the next free `widget-N`), and leave out `col`/`row` to take the first free cell.

Automations are a separate document: `{"schema": 3, "automations": [{"id", "name", "enabled", "trigger", "body": [blocks]}]}`, with triggers such as `{"on": "widget", "widgetId": "..."}`, `{"on": "variables", "names": [...]}`, `{"on": "schedule", "everySeconds": 60}` and `{"on": "manual"}`.

## Stability

Tool names and their arguments are stable. Changes are additive; a renamed tool keeps its old name working for at least one release.
