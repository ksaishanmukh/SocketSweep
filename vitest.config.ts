import { defineConfig } from "vitest/config";

// Kept separate from vite.config.ts, which carries Tauri dev-server settings
// (fixed port, HMR host) that are irrelevant to tests.
//
// Tests run in node. A test that renders React opts into a DOM for itself with
// a `// @vitest-environment jsdom` comment, so the pure-logic tests do not pay
// for one.
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
