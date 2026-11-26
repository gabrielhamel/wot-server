import { ROOT_CONTEXT, SpanKind, SpanStatusCode, trace } from "@opentelemetry/api";
import type { Attributes, Context, Span } from "@opentelemetry/api";
import { W3CTraceContextPropagator } from "@opentelemetry/core";

export type WebContext = "player_administration" | "player_sessions" | "store" | "server";

const propagator = new W3CTraceContextPropagator();

export class WebFailure extends Error {
  constructor(
    message: string,
    readonly outcome: string,
  ) {
    super(message);
  }
}

export function reportFailure(span: Span, outcome: string) {
  span.setAttribute("output.outcome", outcome);
  span.setStatus({ code: SpanStatusCode.ERROR });
  span.addEvent("exception", { "exception.type": outcome });
}

export function webSpan(
  owner: WebContext,
  name: string,
  attributes: Attributes = {},
  parent: Context = ROOT_CONTEXT,
  kind = SpanKind.INTERNAL,
) {
  const span = trace.getTracer("wot-server-web").startSpan(
    name,
    {
      kind,
      attributes: { "ddd.bounded_context.name": owner, ...attributes },
    },
    parent,
  );
  return { span, context: trace.setSpan(parent, span) };
}

export function injectTrace(parent: Context, headers: Headers) {
  propagator.inject(parent, headers, { set: (carrier, key, value) => carrier.set(key, value) });
}

export async function webOperation<T>(
  owner: WebContext,
  name: string,
  attributes: Attributes,
  work: (parent: Context, span: Span) => Promise<T>,
): Promise<T> {
  const { span, context } = webSpan(owner, name, attributes);
  try {
    const result = await work(context, span);
    span.setAttribute("output.outcome", "success");
    return result;
  } catch (error) {
    const outcome = error instanceof WebFailure ? error.outcome : "unexpected_error";
    if (["cancelled", "rejected", "conflict"].includes(outcome)) {
      span.setAttribute("output.outcome", outcome);
    } else {
      reportFailure(span, outcome);
    }
    throw error;
  } finally {
    span.end();
  }
}

export function recordBrowserError(owner: WebContext, source: string, error?: unknown) {
  const { span } = webSpan(owner, "web.exception", { "exception.source": source });
  reportFailure(span, source);
  try {
    if (!(error instanceof Error)) return;
    const type = ["Error", "TypeError", "ReferenceError", "SyntaxError", "RangeError"].find(
      (type) => type === error.name,
    );
    if (type) span.setAttribute("exception.error_class", type);

    // Keep only the bundled source location. Stack lines can include application data.
    const stack = error.stack;
    if (typeof stack !== "string") return;
    const frame = stack
      .slice(0, 4096)
      .split("\n")
      .slice(1)
      .map((line) => /\/assets\/([\w-]+\.js:\d{1,7}:\d{1,7})(?:[)\s]|$)/.exec(line)?.[1])
      .find(Boolean);
    if (frame) span.setAttribute("exception.asset_location", frame);
  } catch {
    // Error subclasses can have throwing getters.
  } finally {
    span.end();
  }
}
