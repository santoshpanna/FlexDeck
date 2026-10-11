# Discord for flexdeck

Mute, deafen, push-to-talk, switch between voice activity and push-to-talk,
pick Discord's microphone and speakers and their volumes, set each person's
volume in the call, join and leave voice channels, open a text channel, play
soundboard sounds, and turn your camera or screen share on and off — and see on
the deck whether you are muted, who is in the call, who is speaking, and how
many notifications came in.

It talks to the **Discord desktop app on this computer** through Discord's local
RPC, so Discord must be running and signed in on the same machine as flexdeck.

## Set up (once, about two minutes)

Discord only lets an *application* control voice once Discord has approved it —
or for the person who made it. flexdeck does not have an approved application
yet, so you make your own; it is yours, it stays on your computer, and nothing
about it is shared.

1. Open the [Discord Developer Portal](https://discord.com/developers/applications)
   and press **New Application**. Call it anything — "flexdeck" is fine.
2. Open **OAuth2** on the left.
   * Copy the **Client ID**.
   * Press **Reset Secret** and copy the **Client Secret**.
   * Under **Redirects**, add `http://127.0.0.1` and save.
3. In flexdeck, open the Discord plugin's settings and paste the Client ID and
   the Client Secret.
4. Press any Discord button on the deck. Discord shows a dialog asking whether
   to let your application in; press **Authorize**. You will not be asked again
   — the plugin keeps its sign-in fresh by itself. To sign out, clear the
   *access token* and *refresh token* settings.

## Push-to-talk

Make a button and, under **While held**, put **Push-to-talk: open the
microphone** on *When pressed* and **Push-to-talk: close the microphone** on
*When let go*. If the tablet loses its connection while you are holding it, the
desktop lets go for you, so your microphone is never left open.

This works by unmuting while held, so set Discord to **voice activity** and
start muted.

## Sliders

**Microphone volume**, **output volume** (to 200%, as in Discord) and **a
person's volume** can each be driven by a slider, and the slider follows changes
made in Discord.

## What Discord allows, honestly

* **Video, screen share and the soundboard** use commands Discord has not made
  public. They are tried, not assumed: if your Discord does not offer one, the
  button says so. Video and screen share may also need extra permissions —
  put `rpc.video.write rpc.screenshare.write` in *extra scopes* if your
  application is allowed them.
* **Server stats** are what Discord's local RPC can see: the server's name, its
  voice channels, and how many people are in them. RPC does not report member
  or online counts; turn on *use guild widget* to read the online count from the
  server's public widget, which the server must have switched on.
* **Notifications** counts the notifications Discord shows on this computer,
  from when the plugin started or was last cleared.
