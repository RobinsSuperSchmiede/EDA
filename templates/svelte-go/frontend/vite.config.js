import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  server: { proxy: { "/api": process.env.BACKEND_URL || "http://127.0.0.1:8000" } },
});
