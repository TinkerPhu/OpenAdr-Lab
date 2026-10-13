import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

export default defineConfig({
  root: resolve(__dirname),
  plugins: [react()],
  // `@lab/ui` is the shared UI source at the repo root (lab-ui/),
  // imported as source by both UIs: one implementation of "how this lab draws
  // a time series", compiled by whichever app is building. Vitest inherits
  // this resolve config, so tests need no separate setup.
  resolve: {
    alias: { "@lab/ui": resolve(__dirname, "../../lab-ui/src") },
  },
  server: {
    port: 5173,
  },
  build: {
    outDir: "dist",
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: "./src/__tests__/setup.ts",
    css: false,
    coverage: {
      provider: "v8",
      reporter: ["text", "html"],
      include: ["src/**/*.{ts,tsx}"],
      exclude: ["src/**/*.test.{ts,tsx}", "src/__tests__/**"],
    },
  },
});
