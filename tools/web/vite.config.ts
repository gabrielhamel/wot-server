import { fileURLToPath } from "node:url";
import { defineConfig } from "vite-plus";

const repo = fileURLToPath(new URL("../../", import.meta.url));

export default defineConfig({
  build: {
    target: "chrome100",
    rolldownOptions: { checks: { moduleLevelDirective: false } },
  },
  oxc: { jsx: { runtime: "automatic" } },
  define: { __WEB_VERSION__: JSON.stringify(process.env.WOT_SERVER_VERSION || "dev") },
  server: {
    host: "127.0.0.1",
    fs: { allow: [repo] },
    proxy: { "/api": "http://127.0.0.1:8080" },
  },
  test: {
    root: repo,
    environment: "jsdom",
    include: ["src/**/*.web.test.tsx"],
    setupFiles: ["tools/web/test/setup.ts"],
    environmentOptions: { jsdom: { url: "http://localhost:8080/" } },
  },
});
