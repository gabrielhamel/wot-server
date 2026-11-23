import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { setupServer } from "msw/node";
import { afterAll, afterEach, beforeAll } from "vite-plus/test";

export const httpServer = setupServer();

beforeAll(() => {
  httpServer.listen({ onUnhandledRequest: "error" });
  window.scrollTo = () => {};
});
afterEach(() => {
  cleanup();
  httpServer.resetHandlers();
  sessionStorage.clear();
  window.history.replaceState(null, "", "/");
});
afterAll(() => httpServer.close());
