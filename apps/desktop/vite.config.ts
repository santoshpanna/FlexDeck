import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // Tauri waits for this exact URL, so the server must not be told to pick a free
  // port when this one is busy.
  clearScreen: false,
  server: {
    // Pinned to IPv4 loopback for the same reason the portal is: `localhost`
    // resolves to `::1` first on Node >= 17, and a client that prefers IPv4 then
    // reports the server as unreachable.
    host: "127.0.0.1",
    // Deliberately NOT 5173. That is the portal's dev server, and the two must be
    // able to run at once while working on the handoff between them.
    port: 5174,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
