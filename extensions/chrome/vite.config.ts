import { resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  define: {
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
  plugins: [react(), tailwindcss()],
  build: {
    target: "chrome116",
    outDir: "dist",
    emptyOutDir: true,
    lib: {
      entry: resolve(__dirname, "src/main.tsx"),
      formats: ["es"],
      fileName: "sidepanel-react",
      cssFileName: "sidepanel-react",
    },
    rollupOptions: {
      output: {
        entryFileNames: "sidepanel-react.js",
      },
    },
  },
});
