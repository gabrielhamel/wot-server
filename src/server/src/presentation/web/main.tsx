import { createRoot } from "react-dom/client";
import { initializeBrowserTelemetry } from "../../../../platform/src/infrastructure/web/browser-telemetry";
import {
  recordBrowserError,
  webOperation,
} from "../../../../platform/src/infrastructure/web/telemetry";

declare const __WEB_MOCK_API__: boolean;

const pages = {
  "/dashboard": {
    owner: "player_administration",
    title: "World of Tanks Server",
    load: () => import("../../../../player-administration/src/web"),
  },
  "/shop": {
    owner: "player_sessions",
    title: "Garage Store",
    load: () => import("../../../../player-sessions/src/web"),
  },
} as const;

const fallback = <p role="alert">Could not load the page. Reload to try again.</p>;

async function start() {
  const route = window.location.pathname.replace(/\/$/, "");
  if (route === "") {
    window.location.replace("/dashboard/" + window.location.search + window.location.hash);
    return;
  }
  const container = document.getElementById("root");
  if (!container) return;
  const page = route === "/dashboard" || route === "/shop" ? pages[route] : undefined;
  if (!page) {
    createRoot(container).render(<p>Page unavailable.</p>);
    return;
  }

  const root = createRoot(container, {
    onCaughtError: (error) => recordBrowserError(page.owner, "react_caught_error", error),
    onUncaughtError: (error) => {
      recordBrowserError(page.owner, "react_uncaught_error", error);
      root.render(fallback);
    },
    onRecoverableError: (error) => recordBrowserError(page.owner, "react_recoverable_error", error),
  });
  document.title = page.title;

  try {
    if (__WEB_MOCK_API__) {
      const { startMockApi } = await import("../../infrastructure/web/mocks/browser");
      await startMockApi(route);
    }
    await initializeBrowserTelemetry(page.owner);
    const { default: Page } = await webOperation(
      page.owner,
      "web.load_page",
      {
        "input.page.route": route,
      },
      () => page.load(),
    );
    root.render(<Page />);
  } catch {
    root.render(fallback);
  }
}

void start();
