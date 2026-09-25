import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: process.env.TAURI_DEV_HOST || undefined,
    port: 1420,
    strictPort: true,
    hmr: process.env.TAURI_DEV_HOST ? { host: process.env.TAURI_DEV_HOST, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    emptyOutDir: true,
    target: "es2022",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (id.includes("node_modules/@xyflow")) return "graph";
          if (["react-markdown", "remark-gfm", "remark-math", "rehype-katex", "katex"].some((name) => id.includes(`node_modules/${name}`))) {
            return "markdown";
          }
          return undefined;
        },
      },
    },
  },
});
