# Soundboard for flexdeck

Play sound clips from the deck — to your own headphones, and into your
microphone so the people in your Discord call or on your stream hear them too.
Optionally, your real microphone is mixed in, so one virtual microphone carries
your voice *and* the clips.

Plays MP3, WAV, FLAC, Ogg Vorbis and AAC/M4A, from a file on this computer or an
`https://` address (downloaded once and kept).

## Hearing clips yourself

Nothing to set up: clips play on Windows' default output, or on the device named
in *monitor device*.

## Playing clips into your microphone

Windows has no way for one program to speak into another program's microphone,
so this uses a **virtual audio cable** — a pretend sound card where whatever
plays into its *input* comes out of its *output*.

1. Install [VB-CABLE](https://vb-audio.com/Cable/) (free; donationware) and
   restart Windows. VoiceMeeter and Virtual Audio Cable work too.
2. Make sure Windows did not make the cable your **default** speakers or
   microphone (Settings → Sound). Installing it sometimes does.
3. In Discord (Settings → Voice & Video) or OBS, choose **CABLE Output** as your
   microphone.
4. In this plugin's settings, turn **mic passthrough** on and pick your real
   microphone in *mic device* (blank: Windows' default microphone).

The plugin finds the cable by itself (and notices if you plug one in later).
Now a clip played with output *both* is heard by you and by everyone listening
to CABLE Output — along with your voice.

Your microphone is mixed into the cable only, never into your own headphones.
It adds about 20–40 ms of delay, and Windows' microphone privacy setting must
let desktop apps use it.

## Actions

| Action | |
| --- | --- |
| Play | Play a clip. *Mode*: **restart** if it is already playing, play another copy (**overlap**), or **toggle** it off. *Output*: both, monitor only, or cable only. |
| Stop / Stop all | |
| Set master volume | Every clip's volume — slider-ready. |
| Set clip volume | One clip's volume, even while it plays — slider-ready. |
| Set mic passthrough | On, off, or toggle. |
| Set mic gain | Your microphone's level in the mix, to 200% — slider-ready. |

A button that plays a clip can show whether it is playing:
`soundboard.clip.<clip>.playing`, where `<clip>` is the path or address exactly
as given. `soundboard.cable.present` lights up when a cable is connected.

## Build

`pnpm --filter @flexdeck/plugin-soundboard build` compiles the release binary and
copies it into `bin/`; `package` then makes the `.fdpkg`. Publishing needs a
Windows build machine, and the binary should be code-signed: an unsigned new
program that opens the microphone is exactly what antivirus software
quarantines.
