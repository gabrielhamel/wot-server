import { SpanStatusCode } from "@opentelemetry/api";
import { ExportResultCode } from "@opentelemetry/core";
import type { ExportResult } from "@opentelemetry/core";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-proto";
import { ProtobufTraceSerializer } from "@opentelemetry/otlp-transformer";
import { expect, it, vi } from "vite-plus/test";
import { captureTraces } from "../../../../tools/web/test/traces";
import { observeBrowserErrors } from "./browser-telemetry";
import { webOperation, webSpan } from "./telemetry";
import { BrowserTraceExporter } from "./trace-export";

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

it("keeps browser trace ancestry when splitting an oversized upload", async () => {
  const transport = vi
    .spyOn(OTLPTraceExporter.prototype, "export")
    .mockImplementation((_spans, complete) => complete({ code: ExportResultCode.SUCCESS }));
  const telemetry = captureTraces();
  try {
    await webOperation("player_administration", "web.admin.list_players", {}, async (parent) => {
      webSpan("player_administration", "web.http.request", {}, parent).span.end();
    });
    const attributes = Object.fromEntries(
      Array.from({ length: 24 }, (_, index) => [`input.fixture_${index}`, "é".repeat(120)]),
    );
    for (let index = 0; index < 16; index++) {
      webSpan("player_administration", "web.exception", attributes).span.end();
    }
    const spans = telemetry.spans();
    expect(ProtobufTraceSerializer.serializeRequest(spans)?.byteLength).toBeGreaterThan(32 * 1024);

    const exporter = new BrowserTraceExporter({ url: "https://example.test/api/web/telemetry" });
    const result = await new Promise<ExportResult>((complete) => exporter.export(spans, complete));
    expect(result.code).toBe(ExportResultCode.SUCCESS);

    const uploads = transport.mock.calls.map(([batch]) => batch);
    expect(uploads.length).toBeGreaterThan(1);
    for (const upload of uploads) {
      expect(ProtobufTraceSerializer.serializeRequest(upload)?.byteLength).toBeLessThanOrEqual(
        32 * 1024,
      );
    }
    expect(uploads.flat()).toEqual(spans);
    const [http, root] = uploads.flat();
    expect(root.name).toBe("web.admin.list_players");
    expect(http.parentSpanContext?.spanId).toBe(root.spanContext().spanId);
    expect(http.spanContext().traceId).toBe(root.spanContext().traceId);
  } finally {
    transport.mockRestore();
    await telemetry.stop();
  }
});
