import { rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { defineConfig, type Plugin } from "vite";

/** public/demo.json is sample data for `pnpm dev` only: copy-public would put it in the release, so it is taken out again. */
const noDemoData = (): Plugin => {
  let out = "";
  return {
    name: "no-demo-data",
    apply: "build",
    configResolved: (c) => {
      out = resolve(c.root, c.build.outDir);
    },
    closeBundle: () => rmSync(join(out, "demo.json"), { force: true }),
  };
};

// Tauri expects a fixed port and no clearing of its own output.
export default defineConfig({
  clearScreen: false,
  plugins: [noDemoData()],
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: { target: "safari15", outDir: "dist", emptyOutDir: true },
});
