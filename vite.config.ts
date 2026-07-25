import { defineConfig } from "vite";

// Tauri sert le frontend depuis un port fixe et surveille lui meme les sources Rust.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    minify: "esbuild",
    sourcemap: false,
    reportCompressedSize: false,
  },
});
