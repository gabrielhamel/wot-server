import { trace } from "@opentelemetry/api";
import { ExportResultCode } from "@opentelemetry/core";
import type { ExportResult } from "@opentelemetry/core";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-proto";
import { ProtobufTraceSerializer } from "@opentelemetry/otlp-transformer";
import { resourceFromAttributes } from "@opentelemetry/resources";
import { BatchSpanProcessor, TracerProvider } from "@opentelemetry/sdk-trace";
import type { ReadableSpan } from "@opentelemetry/sdk-trace";

declare const __WEB_VERSION__: string;

const maxUploadBytes = 32 * 1024;

export class BrowserTraceExporter extends OTLPTraceExporter {
  override export(spans: ReadableSpan[], callback: (result: ExportResult) => void) {
    void this.upload(spans)
      .catch(() => ({ code: ExportResultCode.FAILED }))
      .then((result) => {
        if (result.code !== ExportResultCode.SUCCESS) {
          console.warn("Browser telemetry batch could not be fully exported.", {
            spanCount: spans.length,
          });
        }
        callback(result);
      });
  }

  private async upload(spans: ReadableSpan[]): Promise<ExportResult> {
    if (spans.length === 0) return { code: ExportResultCode.SUCCESS };
    const body = ProtobufTraceSerializer.serializeRequest(spans);
    if (body && body.byteLength <= maxUploadBytes) {
      return new Promise((resolve) => super.export(spans, resolve));
    }

    if (spans.length === 1 || !body) return { code: ExportResultCode.FAILED };
    const midpoint = Math.floor(spans.length / 2);
    const results = await Promise.all([
      this.upload(spans.slice(0, midpoint)),
      this.upload(spans.slice(midpoint)),
    ]);
    return (
      results.find((result) => result.code !== ExportResultCode.SUCCESS) ?? {
        code: ExportResultCode.SUCCESS,
      }
    );
  }
}

export function registerTraceExporter(endpoint: string) {
  const provider = new TracerProvider({
    resource: resourceFromAttributes({
      "service.name": "wot-server-web",
      "service.version": __WEB_VERSION__,
    }),
    spanLimits: { attributeCountLimit: 24, eventCountLimit: 4, attributeValueLengthLimit: 120 },
    spanProcessors: [
      new BatchSpanProcessor({
        exporter: new BrowserTraceExporter({
          url: new URL(endpoint, window.location.origin).href,
          timeoutMillis: 3000,
        }),
        maxQueueSize: 96,
        maxExportBatchSize: 24,
        scheduledDelayMillis: 500,
        exportTimeoutMillis: 3500,
      }),
    ],
  });
  trace.setGlobalTracerProvider(provider);
  window.addEventListener("pagehide", () => {
    void provider.forceFlush().catch(() => {});
  });
}
