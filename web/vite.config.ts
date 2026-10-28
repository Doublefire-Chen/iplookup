import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  server: {
    // `pnpm dev` serves the pages itself and sends data requests to a
    // locally running `iplookup serve`. The pages then have no injected
    // Record, so they fetch it from /json.
    proxy: { "/json": "http://127.0.0.1:18310" },
  },
});
