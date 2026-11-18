import { SpanStatusCode } from "@opentelemetry/api";
import { expect, it } from "vite-plus/test";
import { captureTraces } from "../../../../tools/web/test/traces";
import { observeBrowserErrors } from "./browser-telemetry";

it("exports browser error and rejection diagnostics without their messages or full stack", async () => {
  const telemetry = captureTraces();
  const stop = observeBrowserErrors("player_sessions");
  try {
    const error = new TypeError("secret data in an error");
    error.stack =
      "TypeError: secret data in an error\n    at render (https://example.test/assets/shop-XyZ.js:19:123)\n    at secretFunction";
    window.dispatchEvent(new ErrorEvent("error", { error }));
    const rejection = new Event("unhandledrejection");
    Object.defineProperty(rejection, "reason", { value: "private rejection" });
    window.dispatchEvent(rejection);
    stop();
    window.dispatchEvent(new Event("error"));

    const spans = telemetry.spans();
    expect(spans).toHaveLength(2);
    expect(spans[0].name).toBe("web.exception");
    expect(spans[0].status.code).toBe(SpanStatusCode.ERROR);
    expect(spans[0].attributes).toMatchObject({
      "ddd.bounded_context.name": "player_sessions",
      "exception.source": "window_error",
      "exception.error_class": "TypeError",
      "exception.asset_location": "shop-XyZ.js:19:123",
    });
    expect(spans[1].events[0].attributes?.["exception.type"]).toBe("unhandled_rejection");
    expect(telemetry.serializedSpanData()).not.toMatch(
      /private rejection|secret data|secretFunction|example\.test/,
    );
  } finally {
    stop();
    await telemetry.stop();
  }
});
