import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { httpServer } from "../../../../../tools/web/test/setup";
import { captureTraces } from "../../../../../tools/web/test/traces";
import type { StoreOffer } from "../../../application/store-contracts";
import Shop from "./shop-page";

const token = "x".repeat(43);
const origin = "http://localhost:8080";
const offer: StoreOffer = {
  item: 121,
  name: "Improved Ventilation",
  category: "equipment",
  price: { currency: "credits", amount: 850 },
  in_depot: 0,
  availability: "available",
};
const catalog = { revision: "9", premium_expires_at: null, offers: [offer] };

function openShop(token?: string) {
  window.history.replaceState(null, "", `/shop/${token ? `#${token}` : ""}`);
  return { user: userEvent.setup(), ...render(<Shop />) };
}

afterEach(() => vi.unstubAllGlobals());

describe("Garage Store", () => {
  it("asks for a connected garage when opened without a session", () => {
    openShop();
    expect(screen.getByText("Open the Store from a connected garage.")).toBeVisible();
    expect(screen.queryByRole("button", { name: /buy/i })).toBeNull();
  });

  it("loads offers and buys equipment with a connected trace at each HTTP boundary", async () => {
    const telemetry = captureTraces();
    const notify = vi.fn();
    vi.stubGlobal("jsHostQuery", notify);
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
          offers: [{ ...offer, in_depot: 1, availability: "insufficient_funds" }],
        });
      }),
    );
    try {
      const { user } = openShop(token);
      await user.click(screen.getByRole("tab", { name: "Service" }));
      const buy = await screen.findByRole("button", {
        name: "Buy Improved Ventilation for 850 credits",
      });
      expect(screen.queryByText(/available credits/i)).toBeNull();
      await user.click(buy);
      const pending = screen.getByRole("button", { name: "Buying Improved Ventilation" });
      expect(pending).toBeDisabled();
      expect(screen.getByRole("progressbar")).toBeVisible();
      expect(notify).not.toHaveBeenCalled();
      expect(await screen.findByText("1 in Depot")).toBeVisible();
      expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
      expect(screen.queryByRole("alert")).not.toBeInTheDocument();
      expect(notify).toHaveBeenCalledExactlyOnceWith({
        request: JSON.stringify({
          command: "notification",
          params: {
            type: "PurchaseForCredits",
            message: "Improved Ventilation added to your Depot.",
          },
        }),
      });
      expect(order).toEqual({ item: 121, revision: "9", price: offer.price });
      expect(screen.getByText("1 in Depot")).toBeVisible();
      expect(screen.getByText("Insufficient funds")).toBeVisible();
      expect(buy).toBeDisabled();
      telemetry.expectRequest(catalogTrace, "web.store.load_catalog", "/api/shop/catalog", 200);
      const { parent } = telemetry.expectRequest(
        orderTrace,
        "web.store.purchase",
        "/api/shop/orders",
        200,
      );
      expect(parent?.attributes).toMatchObject({
        "ddd.bounded_context.name": "player_sessions",
        "input.item": 121,
        "input.revision": "9",
        "input.price.currency": "credits",
        "input.price.amount": 850,
        "output.offer_count": 1,
        "output.notification": "submitted",
      });
      expect(telemetry.serializedSpanData()).not.toContain(token);
    } finally {
      await telemetry.stop();
    }
  });

  it("shows a stale-price conflict and reloads before retrying the purchase", async () => {
    const telemetry = captureTraces();
    const notify = vi.fn();
    vi.stubGlobal("jsHostQuery", notify);
    const orders: unknown[] = [];
    let conflictTrace: string | null = null;
    httpServer.use(
      http.get(`${origin}/api/shop/catalog`, () =>
        HttpResponse.json(
          orders.length
            ? {
                ...catalog,
                revision: "10",
                offers: [{ ...offer, price: { currency: "credits", amount: 950 } }],
              }
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
          offers: [{ ...offer, price: { currency: "credits", amount: 950 }, in_depot: 1 }],
        });
      }),
    );
    try {
      const { user } = openShop(token);
      await user.click(screen.getByRole("tab", { name: "Service" }));
      const buy = await screen.findByRole("button", { name: /buy improved ventilation/i });
      await user.click(buy);
      expect(await screen.findByText(/the offer or account changed/i)).toBeVisible();
      expect(buy).toBeDisabled();
      expect(notify).not.toHaveBeenCalled();
      await user.click(screen.getByRole("button", { name: "Reload offers" }));
      await user.click(
        await screen.findByRole("button", { name: "Buy Improved Ventilation for 950 credits" }),
      );
      expect(await screen.findByText("1 in Depot")).toBeVisible();
      expect(orders).toEqual([
        { item: 121, revision: "9", price: offer.price },
        { item: 121, revision: "10", price: { currency: "credits", amount: 950 } },
      ]);
      const { http: span } = telemetry.expectRequest(
        conflictTrace,
        "web.store.purchase",
        "/api/shop/orders",
        409,
      );
      expect(span?.attributes["output.outcome"]).toBe("conflict");
    } finally {
      await telemetry.stop();
    }
  });

  it("activates and extends Premium Account time using the selected gold offer", async () => {
    const premium: StoreOffer = {
      ...offer,
      item: -1,
      name: "1 day",
      category: "premium_account",
      price: { currency: "gold", amount: 250 },
    };
    const expiry = Math.floor(Date.now() / 1000) + 86_400;
    const orders: unknown[] = [];
    httpServer.use(
      http.get(`${origin}/api/shop/catalog`, () =>
        HttpResponse.json({ ...catalog, offers: [premium] }),
      ),
      http.post(`${origin}/api/shop/orders`, async ({ request }) => {
        orders.push(await request.json());
        return HttpResponse.json({
          revision: String(9 + orders.length),
          premium_expires_at: expiry + (orders.length - 1) * 86_400,
          offers: [premium],
        });
      }),
    );
    const { user } = openShop(token);
    const buy = await screen.findByRole("button", { name: "Buy 1 day for 250 gold" });
    expect(screen.getByRole("heading", { level: 1, name: "WoT Premium Account" })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Store" })).not.toBeInTheDocument();
    await user.click(buy);
    const firstExpiry = await screen.findByText(/active until/i);
    const firstLabel = firstExpiry.textContent;
    await user.click(buy);
    await waitFor(() => expect(firstExpiry.textContent).not.toBe(firstLabel));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(orders).toEqual([
      { item: -1, price: premium.price, revision: "9" },
      { item: -1, price: premium.price, revision: "10" },
    ]);
  });

  it("filters Collectors’ vehicles and explains the research lock", async () => {
    const vehicles: StoreOffer[] = [
      {
        ...offer,
        item: 1,
        name: "Panther/M10",
        category: "premium_vehicles",
        nation: "germany",
        tier: 7,
      },
      {
        ...offer,
        item: 2,
        name: "Jagdpanther II",
        category: "collectors_vehicles",
        nation: "germany",
        tier: 8,
        availability: "research_required",
      },
      {
        ...offer,
        item: 3,
        name: "KV-85",
        category: "collectors_vehicles",
        nation: "ussr",
        tier: 6,
      },
    ];
    httpServer.use(
      http.get(`${origin}/api/shop/catalog`, () =>
        HttpResponse.json({ ...catalog, offers: vehicles }),
      ),
    );
    const { user } = openShop(token);
    await user.click(screen.getByRole("tab", { name: "Vehicles" }));
    expect(await screen.findByRole("heading", { name: "Panther/M10" })).toBeVisible();
    await user.click(screen.getByRole("tab", { name: "Collectors’ vehicles" }));
    await user.click(screen.getByRole("combobox", { name: "Nation" }));
    await user.click(screen.getByRole("option", { name: "Germany" }));
    expect(screen.queryByRole("heading", { name: "KV-85" })).toBeNull();
    expect(screen.getByRole("button", { name: /buy jagdpanther ii/i })).toBeDisabled();
    expect(screen.getByText(/research a tech tree vehicle/i)).toBeVisible();
  });
});
