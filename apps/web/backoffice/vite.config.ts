import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// In development, the API is the cell's: `KEEL_CELL` is where it listens.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: { "/api": process.env.KEEL_CELL ?? "http://127.0.0.1:8080" },
  },
  build: { outDir: "dist", sourcemap: true },
});
