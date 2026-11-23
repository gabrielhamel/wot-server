import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite-plus";

const repo = fileURLToPath(new URL("../..", import.meta.url));
const require = createRequire(import.meta.url);

export default defineConfig(({ command, isPreview, mode }) => {
  const mockApi = command === "serve" && !isPreview && mode !== "server";

  return {
    lint: {
      jsPlugins: ["./bounded-context-imports.ts"],
      rules: { "web/published-context-import": "error" },
    },
    plugins: [
      {
        name: "msw-development-worker",
        apply: () => mockApi,
        configureServer(server) {
          const worker = readFileSync(require.resolve("msw/mockServiceWorker.js"));
          server.middlewares.use((request, response, next) => {
            if (request.url !== "/mockServiceWorker.js") return next();
            response.setHeader("Content-Type", "text/javascript; charset=utf-8");
            response.setHeader("Cache-Control", "no-store");
            response.end(worker);
          });
        },
      },
    ],
    build: {
      target: "chrome100",
      rolldownOptions: { checks: { moduleLevelDirective: false } },
    },
    oxc: { jsx: { runtime: "automatic" } },
    define: {
      __WEB_VERSION__: JSON.stringify(process.env.WOT_SERVER_VERSION || "dev"),
      __WEB_MOCK_API__: JSON.stringify(mockApi),
    },
    server: {
      host: "127.0.0.1",
      fs: { allow: [repo] },
      proxy: mode === "server" ? { "/api": "http://127.0.0.1:8080" } : undefined,
    },
    test: {
      root: repo,
      environment: "jsdom",
      include: ["{src,crates}/**/*.web.test.tsx"],
      setupFiles: ["tools/web/test/setup.ts"],
      environmentOptions: { jsdom: { url: "http://localhost:8080/" } },
    },
  };
});
