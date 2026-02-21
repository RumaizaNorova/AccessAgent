import { defineConfig } from "vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    // HMR off = no auto-reload on file save. Prevents interrupting the 2.3GB model download.
    // Set to true for hot reload during development after the model is cached.
    hmr: false,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
