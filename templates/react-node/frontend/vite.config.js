import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// OpenShell passes the backend URL (dynamic port) as BACKEND_URL.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: { "/api": process.env.BACKEND_URL || "http://127.0.0.1:8000" },
  },
});
