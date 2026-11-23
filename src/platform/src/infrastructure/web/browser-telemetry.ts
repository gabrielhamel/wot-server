import { recordBrowserError } from "./telemetry";
import type { WebContext } from "./telemetry";

const endpoint = "/api/web/telemetry";

export function observeBrowserErrors(owner: WebContext, target: Window = window) {
  const onError = (event: Event) => {
    if (event instanceof ErrorEvent) recordBrowserError(owner, "window_error", event.error);
    else recordBrowserError(owner, "resource_error");
  };
  const onRejection = (event: PromiseRejectionEvent) =>
    recordBrowserError(owner, "unhandled_rejection", event.reason);
  target.addEventListener("error", onError, true);
  target.addEventListener("unhandledrejection", onRejection);
  return () => {
    target.removeEventListener("error", onError, true);
    target.removeEventListener("unhandledrejection", onRejection);
  };
}

export async function initializeBrowserTelemetry(owner: WebContext) {
  // Keep the optional collector and exporter download off the page's critical path after the deadline.
  const discovery = new AbortController();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<void>((resolve) => {
    timer = setTimeout(() => {
      discovery.abort();
      resolve();
    }, 750);
  });
  const initialize = async () => {
    const response = await fetch(endpoint, { cache: "no-store", signal: discovery.signal });
    if (response.status !== 204) return;
    const { registerTraceExporter } = await import("./trace-export");
    if (discovery.signal.aborted) return;
    registerTraceExporter(endpoint);
    observeBrowserErrors(owner);
  };
  try {
    await Promise.race([initialize(), deadline]);
  } catch {
    // The UI can start when telemetry discovery or registration fails.
  } finally {
    clearTimeout(timer);
  }
}
