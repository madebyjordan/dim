import { fileURLToPath, URL } from "node:url";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const sourceDirectories = [
  "actions",
  "api",
  "assets",
  "Components",
  "Helpers",
  "hooks",
  "Pages",
  "slices",
  "Themes",
];

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: sourceDirectories.map((directory) => ({
      find: directory,
      replacement: fileURLToPath(
        new URL(`./src/${directory}`, import.meta.url)
      ),
    })),
  },
  server: {
    port: 3000,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8000",
        changeOrigin: true,
      },
      "/images": {
        target: "http://127.0.0.1:8000",
        changeOrigin: true,
      },
      "/ws": {
        target: "http://127.0.0.1:8000",
        changeOrigin: true,
        rewriteWsOrigin: true,
        ws: true,
      },
    },
  },
  build: {
    assetsDir: "static",
    outDir: "build",
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: "./src/setupTests.js",
  },
});
