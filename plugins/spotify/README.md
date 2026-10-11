# Spotify for flexdeck

Like or unlike the song that is playing, add it to a playlist, start a playlist,
album or artist, set Spotify's own volume (a slider can drive it), move playback
to another device — and show the track, the artist, whether you liked it, and
its **album art** on the deck.

Play, pause and skip need no plugin: the built-in **Media** plugin controls
Spotify, TIDAL, Apple Music and any other player, and shows what each is
playing, without signing in to anything.

## Set up (once, about two minutes)

Spotify only lets an application in development mode serve the accounts it
lists, so you use your own:

1. Open the [Spotify Developer Dashboard](https://developer.spotify.com/dashboard)
   and press **Create app**. Any name and description will do.
2. Under **Redirect URIs** add exactly `http://127.0.0.1:47823/callback`.
   (Spotify needs the loopback address written out, not `localhost`.) Tick
   **Web API** and save.
3. Copy the app's **Client ID** into this plugin's settings in flexdeck. There
   is no secret to copy: sign-in uses PKCE.
4. Put **Connect** on a button and press it. Your browser opens Spotify's
   consent page; agree, and the tab tells you flexdeck is connected. The sign-in
   is kept and renewed by the plugin. To sign out, clear the *refresh token*
   setting.

If port 47823 is taken on your computer, choose another in *redirect port* and
use the same number in the Redirect URI.

## Notes

* Controlling playback — volume, starting a playlist, moving to a device — needs
  **Spotify Premium**; the button says so on a free account. Liking songs and
  adding them to playlists work on any account.
* The player is read at most every three seconds, and only while something on
  the deck shows it. If Spotify asks the plugin to slow down, it does.
* `spotify.art` holds the album art for a widget's icon: set the widget's icon
  to follow `{{spotify.art}}`.
