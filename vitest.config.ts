import { defineConfig } from "vitest/config";

// Kept separate from vite.config.ts, which carries Tauri dev-server settings
// (fixed port, HMR host) that are irrelevant to tests.
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
