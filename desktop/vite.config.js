import { defineConfig } from "vite";
import { resolve } from "path";

export default defineConfig({
  root: "src",
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    rollupOptions: {
      // Two entry points: the launcher window and the main vault window.
      input: {
        launcher: resolve(__dirname, "src/launcher.html"),
        index: resolve(__dirname, "src/index.html"),
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
  },
  clearScreen: false,
});
