# OBS Studio for flexdeck

Switch scenes, record, stream, save the replay, mute inputs and set their volume
from the deck — and show what OBS is doing: the live scene, a red light while
recording, the recording's running time.

## Set up (once)

1. In OBS, open **Tools → WebSocket Server Settings**, tick **Enable WebSocket
   server**, and note the port (4455 unless you changed it). If **Enable
   Authentication** is ticked, press **Show Connect Info** and copy the password.
2. In flexdeck, open this plugin's settings and fill in the port and the
   password. Leave the host at `127.0.0.1` when OBS runs on the same computer as
   flexdeck; otherwise put that computer's address.

The plugin connects when OBS starts and reconnects on its own when OBS restarts.
`obs.connected` says whether it is connected right now.

## What it offers

**Actions:** switch the live scene; put a scene in preview and send it live
(studio mode); start, stop, pause or toggle recording and streaming; save the
replay buffer; toggle the virtual camera; mute, unmute or toggle an input; set an
input's volume — which a **slider** can drive, with a tap on its name to mute.

Scene and input names are offered from OBS as you pick them, so they are spelled
the way OBS spells them.

**Live values** for labels and lights:

| Variable | |
| --- | --- |
| `obs.connected` | Whether OBS is connected |
| `obs.scene` | The live scene |
| `obs.recording`, `obs.record_paused` | Whether it is recording, and paused |
| `obs.record_time` | How long it has recorded, `HH:MM:SS` |
| `obs.streaming`, `obs.stream_time` | Whether it is live, and for how long |
| `obs.studio_mode` | Whether studio mode is on |
| `obs.input.<name>.volume` | An input's volume, 0–100, for a slider's thumb |

A record button that lights while recording and shows the time: set **Light up
while** to `obs.recording` and **Label shows** to `REC {{obs.record_time}}`.

**Events** for automations: OBS connected or went away, the live scene changed,
recording or streaming started or stopped, an input was muted or unmuted.

## Building it

```sh
pnpm test            # the tests, against a fake OBS
pnpm run package     # writes obs-<version>.fdpkg
```
