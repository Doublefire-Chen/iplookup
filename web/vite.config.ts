import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  server: {
    // `pnpm dev` talks to a locally running `iplookup serve`.
    proxy: { "/json": "http://127.0.0.1:18310" },
  },
});
