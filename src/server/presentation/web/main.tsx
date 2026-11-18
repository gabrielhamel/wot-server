import { createRoot } from "react-dom/client";
import { initializeBrowserTelemetry } from "../../../platform/infrastructure/web/browser-telemetry";
import { recordBrowserError, webOperation } from "../../../platform/infrastructure/web/telemetry";

const pages = {
  "/admin": {
    owner: "player_administration",
    title: "Server Administration",
    load: () => import("../../../player_administration/presentation/web/admin-page"),
  },
  "/shop": {
    owner: "player_sessions",
    title: "Garage Store",
    load: () => import("../../../player_sessions/presentation/store/web/shop-page"),
  },
} as const;

const fallback = <p role="alert">Could not load the page. Reload to try again.</p>;

async function start() {
  const route = window.location.pathname.replace(/\/$/, "");
  if (route === "") {
    window.location.replace("/admin/" + window.location.hash);
    return;
  }
  const container = document.getElementById("root");
  if (!container) return;
  const page = route === "/admin" || route === "/shop" ? pages[route] : undefined;
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

  await initializeBrowserTelemetry(page.owner);
  try {
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
