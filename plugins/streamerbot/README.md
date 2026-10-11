# Streamer.bot for flexdeck

Run [Streamer.bot](https://streamer.bot) actions from the deck, flip between two
with one button, switch actions on and off, and show on the deck whether
Streamer.bot is connected, whether an action is enabled, and the value of any
global variable.

## Set up

1. In Streamer.bot, open **Servers/Clients → WebSocket Server**, tick
   **Auto Start**, and press **Start Server**. The defaults (`127.0.0.1`, port
   `8080`, endpoint `/`) are what this plugin uses unless told otherwise.
2. If you turned on **Authentication**, put the same password in this plugin's
   settings in flexdeck.

That is all you need to run actions.

## Switching actions on and off

Streamer.bot's WebSocket API can run an action but has no request to enable or
disable one, so **Enable action**, **Disable action** and **Toggle action** run
a small helper action instead. Import it once:

1. Create an action called **`flexdeck: set action state`** (or any name — then
   set the same name as *helper action* in this plugin's settings).
2. Give it one sub-action: **Core → C# → Execute C# Code**, with this code:

   ```csharp
   using System;

   public class CPHInline
   {
       // flexdeck sends: actionId, actionName, enabled (true or false).
       public bool Execute()
       {
           if (!CPH.TryGetArg("actionName", out string name) || string.IsNullOrEmpty(name))
               return false;
           CPH.TryGetArg("enabled", out object raw);
           bool enabled = raw is bool b
               ? b
               : string.Equals(raw?.ToString(), "true", StringComparison.OrdinalIgnoreCase);
           if (enabled)
               CPH.EnableAction(name);
           else
               CPH.DisableAction(name);
           return true;
       }
   }
   ```

3. Press **Compile**, then **Save and Compile**.

## What you get

| Action | What it does |
| --- | --- |
| Run action | Runs an action, optionally with arguments (`user=ada;message=hi`). |
| Run action with a value | Runs an action with a number as one argument — bind it to a **slider** to send 0–100 as you drag. |
| Switch | Runs the first action, then the second, then the first again. It reports `a` or `b`, so a button's look can follow which side is on. |
| Enable / Disable / Toggle action | Switches an action on or off (needs the helper above). |

| Variable | |
| --- | --- |
| `streamerbot.connected` | Whether Streamer.bot is connected — bind a button's look to it as a status light. |
| `streamerbot.action.<name>.enabled` | One per action: whether it is enabled. |
| `streamerbot.switch.<key>` | Which side a switch is on. |
| `streamerbot.global.<name>` | A global variable, for each name listed in the *globals* setting. |

| Event | |
| --- | --- |
| `action_completed` | An action finished. |
| `custom` | A **General → Custom** event was broadcast; its data is passed on as JSON. |
| `global_changed` | A global variable changed (on Streamer.bot versions that announce it). |
| `connected`, `disconnected` | The connection came and went. |
