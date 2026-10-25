import preact from "@preact/preset-vite";
import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

// One self-contained index.html, embedded into mew.exe by src/ui.rs.
export default defineConfig({
  plugins: [preact(), viteSingleFile()],
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
