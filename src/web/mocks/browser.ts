import { http, HttpResponse } from "msw";
import { setupWorker } from "msw/browser";
import { adminHandlers } from "./admin";
import { demoStoreToken, storeHandlers } from "./store";

export async function startMockApi(route: string) {
  const worker = setupWorker(
    ...adminHandlers(),
    ...storeHandlers(),
    http.get("/api/web/telemetry", () => new HttpResponse(null, { status: 404 })),
    http.all("/api/*", () =>
      HttpResponse.json({ message: "No mock API handler." }, { status: 501 }),
    ),
  );
  await worker.start();

  if (route === "/shop") {
    const url = new URL(window.location.href);
    url.hash = demoStoreToken;
    if (url.hash !== window.location.hash) {
      window.history.replaceState(window.history.state, "", url);
    }
  }
}
