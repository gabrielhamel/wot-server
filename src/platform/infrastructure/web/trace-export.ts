import { trace } from "@opentelemetry/api";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-proto";
import { resourceFromAttributes } from "@opentelemetry/resources";
import { BatchSpanProcessor, TracerProvider } from "@opentelemetry/sdk-trace";

declare const __WEB_VERSION__: string;

export function registerTraceExporter(endpoint: string) {
  const provider = new TracerProvider({
    resource: resourceFromAttributes({
      "service.name": "wot-server-web",
      "service.version": __WEB_VERSION__,
    }),
    spanLimits: { attributeCountLimit: 24, eventCountLimit: 4, attributeValueLengthLimit: 120 },
    spanProcessors: [
      new BatchSpanProcessor({
        exporter: new OTLPTraceExporter({
          url: new URL(endpoint, window.location.origin).href,
          timeoutMillis: 3000,
        }),
        maxQueueSize: 96,
        maxExportBatchSize: 24,
        scheduledDelayMillis: 1800,
        exportTimeoutMillis: 3500,
      }),
    ],
  });
  trace.setGlobalTracerProvider(provider);
  window.addEventListener("pagehide", () => {
    void provider.forceFlush().catch(() => {});
  });
}
