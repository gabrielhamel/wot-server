import { SpanKind } from "@opentelemetry/api";
import type { Context } from "@opentelemetry/api";
import { injectTrace, reportFailure, WebFailure, webSpan } from "./telemetry";
import type { WebContext } from "./telemetry";

class HttpFailure extends WebFailure {
  constructor(
    message: string,
    readonly status: number,
  ) {
    super(message, status === 409 ? "conflict" : status < 500 ? "rejected" : "server_error");
  }
}

export function responseStatus(error: unknown) {
  return error instanceof HttpFailure ? error.status : undefined;
}

export function errorMessage(error: unknown, fallback = "The request could not be completed.") {
  return error instanceof Error && error.message ? error.message : fallback;
}

interface WebRequest {
  owner: WebContext;
  route: string;
  path: string;
  method: "GET" | "POST";
  parent: Context;
  token?: string | null;
  body?: object;
  signal?: AbortSignal;
  errorFormat: "json" | "text";
}

async function readError(response: Response, format: WebRequest["errorFormat"]) {
  const fallback = "The server returned an unexpected response.";
  if (format === "text") return (await response.text()) || fallback;
  const result: unknown = await response.json().catch(() => null);
  return typeof result === "object" &&
    result !== null &&
    "message" in result &&
    typeof result.message === "string"
    ? result.message
    : fallback;
}

export async function request<T>(options: WebRequest): Promise<T> {
  const { owner, route, path, method, parent, token, body, signal, errorFormat } = options;
  const { span, context } = webSpan(
    owner,
    "web.http.request",
    {
      "http.request.method": method,
      "http.route": route,
    },
    parent,
    SpanKind.CLIENT,
  );
  try {
    const headers = new Headers({ Accept: "application/json" });
    if (token) headers.set("Authorization", `Bearer ${token}`);
    const payload = body === undefined ? undefined : JSON.stringify(body);
    if (payload !== undefined) {
      headers.set("Content-Type", "application/json");
      span.setAttribute("http.request.body.size", new TextEncoder().encode(payload).length);
    }
    injectTrace(context, headers);

    const response = await fetch(new URL(path, window.location.origin), {
      method,
      headers,
      body: payload,
      signal,
      cache: "no-store",
      redirect: "error",
    });
    span.setAttribute("http.response.status_code", response.status);
    if (!response.ok)
      throw new HttpFailure(await readError(response, errorFormat), response.status);
    const result: unknown = response.status === 204 ? undefined : await response.json();
    if (result === null)
      throw new WebFailure("The server returned an unexpected response.", "invalid_response");
    span.setAttribute("output.outcome", "success");
    return result as T;
  } catch (error) {
    const failure = signal?.aborted
      ? new WebFailure("The request was cancelled.", "cancelled")
      : error instanceof WebFailure
        ? error
        : error instanceof SyntaxError
          ? new WebFailure("The server returned an unexpected response.", "invalid_response")
          : new WebFailure("Could not reach the server.", "network_error");
    if (["cancelled", "rejected", "conflict"].includes(failure.outcome)) {
      span.setAttribute("output.outcome", failure.outcome);
    } else {
      reportFailure(span, failure.outcome);
    }
    throw failure;
  } finally {
    span.end();
  }
}
