import { defineConfig } from "vitest/config";

// Pure modules run in Node; DOM tests opt in with a `// @vitest-environment happy-dom` comment.
export default defineConfig({
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
    // Dates and numbers format by the machine's locale and timezone; pin both so a run is the same anywhere.
    env: { TZ: "UTC", LC_ALL: "en_US.UTF-8", LANG: "en_US.UTF-8" },
    // Tests that read the stylesheet's own text (its colors) get it; every other CSS file is left empty.
    css: { include: [/src\/style\.css/] },
    restoreMocks: true,
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts"],
      // fonts.ts is CSS imports; main.ts is boot wiring (Tauri events), exercised by running the app.
      exclude: ["src/**/*.test.ts", "src/test/**", "src/fonts.ts", "src/main.ts", "src/vite-env.d.ts"],
      reporter: ["text", "text-summary"],
      // Just under today's numbers (about 84% of lines), so a drop fails `test:coverage`.
      thresholds: { lines: 82 },
    },
  },
});
