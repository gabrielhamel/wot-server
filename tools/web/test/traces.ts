import { trace } from "@opentelemetry/api";
import {
  InMemorySpanExporter,
  SimpleSpanProcessor,
  TracerProvider,
} from "@opentelemetry/sdk-trace";
import { expect } from "vite-plus/test";

export function captureTraces() {
  const exporter = new InMemorySpanExporter();
  const provider = new TracerProvider({ spanProcessors: [new SimpleSpanProcessor({ exporter })] });
  trace.setGlobalTracerProvider(provider);

  return {
    spans: () => exporter.getFinishedSpans(),
    serializedSpanData() {
      return JSON.stringify(
        exporter.getFinishedSpans().map((span) => ({
          name: span.name,
          attributes: span.attributes,
          events: span.events,
          status: span.status,
        })),
      );
    },
    expectRequest(header: string | null, operation: string, route: string, status: number) {
      const spans = exporter.getFinishedSpans();
      const http = spans.find(
        (span) => `00-${span.spanContext().traceId}-${span.spanContext().spanId}-01` === header,
      );
      expect(http).toBeDefined();
      expect(http?.name).toBe("web.http.request");
      expect(http?.attributes).toMatchObject({
        "http.route": route,
        "http.response.status_code": status,
      });
      const parent = spans.find(
        (span) => span.spanContext().spanId === http?.parentSpanContext?.spanId,
      );
      expect(parent?.name).toBe(operation);
      expect(parent?.spanContext().traceId).toBe(http?.spanContext().traceId);
      return { http, parent };
    },
    async stop() {
      trace.disable();
      await provider.shutdown();
    },
  };
}
