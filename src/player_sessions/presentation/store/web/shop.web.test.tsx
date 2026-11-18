import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { describe, expect, it } from "vite-plus/test";
import { httpServer } from "../../../../../tools/web/test/setup";
import { captureTraces } from "../../../../../tools/web/test/traces";
import Shop from "./shop-page";

const token = "x".repeat(43);
const origin = "http://localhost:8080";
const offer = { item: 121, name: "Improved Ventilation", credits: 850, in_depot: 0 };
const catalog = { revision: "9", credits: 1000, offers: [offer] };

function openShop(token?: string) {
  window.history.replaceState(null, "", `/shop/${token ? `#${token}` : ""}`);
  return { user: userEvent.setup(), ...render(<Shop />) };
}

describe("Garage Store", () => {
  it("asks for a connected garage when opened without a session", () => {
    openShop();
    expect(screen.getByRole("heading", { name: "Equipment" })).toBeVisible();
    expect(screen.getByText("Open the Store from a connected garage.")).toBeVisible();
    expect(screen.queryByRole("button", { name: /buy/i })).toBeNull();
  });

  it("loads offers and buys equipment with a connected trace at each HTTP boundary", async () => {
    const telemetry = captureTraces();
    let orderTrace: string | null = null;
    let catalogTrace: string | null = null;
    let order: unknown;
    httpServer.use(
      http.get(`${origin}/api/shop/catalog`, ({ request }) => {
        if (request.headers.get("authorization") !== `Bearer ${token}`) {
          return new HttpResponse("Invalid session", { status: 401 });
        }
        catalogTrace = request.headers.get("traceparent");
        return HttpResponse.json(catalog);
      }),
      http.post(`${origin}/api/shop/orders`, async ({ request }) => {
        if (request.headers.get("authorization") !== `Bearer ${token}`) {
          return new HttpResponse("Invalid session", { status: 401 });
        }
        order = await request.json();
        orderTrace = request.headers.get("traceparent");
        return HttpResponse.json({
          ...catalog,
          revision: "10",
          credits: 150,
          offers: [{ ...offer, in_depot: 1 }],
        });
      }),
    );
    try {
      const { user } = openShop(token);
      const buy = await screen.findByRole("button", {
        name: "Buy Improved Ventilation for 850 credits",
      });
      expect(screen.getByText("1,000")).toBeVisible();
      await user.click(buy);
      expect(await screen.findByText("Improved Ventilation added to your Depot.")).toBeVisible();
      expect(order).toEqual({ item: 121, revision: "9", credits: 850 });
      expect(screen.getByText("1 in Depot")).toBeVisible();
      expect(screen.getByText("150")).toBeVisible();
      expect(buy).toBeDisabled();
      telemetry.expectRequest(catalogTrace, "web.store.load_catalog", "/api/shop/catalog", 200);
      const { parent } = telemetry.expectRequest(
        orderTrace,
        "web.store.purchase_equipment",
        "/api/shop/orders",
        200,
      );
      expect(parent?.attributes).toMatchObject({
        "ddd.bounded_context.name": "player_sessions",
        "input.item": 121,
        "input.revision": "9",
        "output.offer_count": 1,
      });
      expect(telemetry.serializedSpanData()).not.toContain(token);
    } finally {
      await telemetry.stop();
    }
  });

  it("shows a stale-price conflict and reloads before retrying the purchase", async () => {
    const telemetry = captureTraces();
    const orders: unknown[] = [];
    let conflictTrace: string | null = null;
    httpServer.use(
      http.get(`${origin}/api/shop/catalog`, () =>
        HttpResponse.json(
          orders.length
            ? { ...catalog, revision: "10", offers: [{ ...offer, credits: 950 }] }
            : catalog,
        ),
      ),
      http.post(`${origin}/api/shop/orders`, async ({ request }) => {
        orders.push(await request.json());
        if (orders.length === 1) {
          conflictTrace = request.headers.get("traceparent");
          return new HttpResponse(
            "The offer or account changed. Review the current price and try again.",
            { status: 409 },
          );
        }
        return HttpResponse.json({
          ...catalog,
          credits: 50,
          offers: [{ ...offer, credits: 950, in_depot: 1 }],
        });
      }),
    );
    try {
      const { user } = openShop(token);
      const buy = await screen.findByRole("button", { name: /buy improved ventilation/i });
      await user.click(buy);
      expect(await screen.findByText(/the offer or account changed/i)).toBeVisible();
      expect(buy).toBeDisabled();
      await user.click(screen.getByRole("button", { name: "Reload offers" }));
      await user.click(
        await screen.findByRole("button", { name: "Buy Improved Ventilation for 950 credits" }),
      );
      expect(await screen.findByText("1 in Depot")).toBeVisible();
      expect(orders).toEqual([
        { item: 121, revision: "9", credits: 850 },
        { item: 121, revision: "10", credits: 950 },
      ]);
      const { http: span } = telemetry.expectRequest(
        conflictTrace,
        "web.store.purchase_equipment",
        "/api/shop/orders",
        409,
      );
      expect(span?.attributes["output.outcome"]).toBe("conflict");
    } finally {
      await telemetry.stop();
    }
  });
});
