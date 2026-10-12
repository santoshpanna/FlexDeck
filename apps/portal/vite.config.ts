import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  server: {
    // Bound to IPv4 loopback explicitly, NOT left to the default.
    //
    // The default host is `localhost`, which Node resolves to `::1` first since
    // v17 — so Vite ends up listening on IPv6 loopback ONLY, and
    // `http://127.0.0.1:5173` is refused outright. Whether that bites depends
    // on the client: Chrome and PowerShell pick `::1` and work, while Firefox
    // (and Zen) prefer IPv4 for `localhost` and report that the server is
    // unreachable. Pinning `127.0.0.1` makes both families work.
    //
    // Deliberately not `host: true`, which would bind every interface and put a
    // dev server that relays a bearer-token API onto the LAN.
    host: "127.0.0.1",

    // Pinned, and it must not move. `dev:web` starts the cloud, waits for Vite to
    // answer on this exact port, and prints that port to the user. Vite's default
    // when a port is busy is to take the NEXT free one — which silently made that
    // banner a lie, and (observed for real) let the portal take 5174, the desktop
    // app's dev server. Failing loudly is the only safe behaviour here.
    port: 5173,
    strictPort: true,

    // Development only. In production the cloud serves this bundle from the
    // same origin as the API, so there is no proxy and no second server.
    //
    // The cloud listens on plain HTTP at 127.0.0.1:8787 — see `DEFAULT_PORT` in
    // `crates/flexdeck-cloud/src/bin/flexdeck-cloud.rs`. It binds loopback by
    // default, so this relays to a service only this machine can reach. The
    // earlier target here was the *agent*, which served this bundle before the
    // cloud existed; the portal no longer talks to an agent directly.
    //
    // `ws: true` because the portal's live socket (`/api/live`) comes through
    // here too. Without it the upgrade is never passed on: requests work, and
    // nothing the desktop pushes — slider values, looks, pages — ever arrives.
    proxy: {
      "/api": { target: "http://127.0.0.1:8787", ws: true },
    },
  },
});
