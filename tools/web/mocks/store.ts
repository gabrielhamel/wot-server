import { http, HttpResponse } from "msw";
import type { StoreCatalog, StoreOrder } from "../../../src/player_sessions/application/store-contracts";

export const demoStoreToken = "demo".padEnd(43, "_");

export function storeHandlers() {
  let revision = 1;
  const catalog: StoreCatalog = {
    revision: String(revision),
    credits: 900_000,
    offers: [
      { item: 121, name: "Improved Ventilation Class 3", credits: 600_000, in_depot: 0 },
      { item: 122, name: "Improved Ventilation Class 2", credits: 200_000, in_depot: 1 },
      { item: 123, name: "Turbocharger Class 1", credits: 600_000, in_depot: 0 },
      { item: 124, name: "Turbocharger Class 2", credits: 200_000, in_depot: 0 },
    ],
  };

  return [
    http.all("/api/shop/*", ({ request }) => {
      if (request.headers.get("Authorization") !== `Bearer ${demoStoreToken}`) {
        return HttpResponse.text("Open the Store from a connected garage.", { status: 401 });
      }
    }),
    http.get("/api/shop/catalog", () => HttpResponse.json(catalog)),
    http.post("/api/shop/orders", async ({ request }) => {
      const order = (await request.json()) as StoreOrder;
      const offer = catalog.offers.find(({ item }) => item === order.item);
      if (!offer) return HttpResponse.text("This offer is unavailable.", { status: 404 });
      if (order.revision !== catalog.revision || order.credits !== offer.credits) {
        return HttpResponse.text("The offer or account changed. Review the current price and try again.", {
          status: 409,
        });
      }
      if (catalog.credits < offer.credits) {
        return HttpResponse.text("You need more credits for this offer.", { status: 422 });
      }
      catalog.credits -= offer.credits;
      offer.in_depot += 1;
      catalog.revision = String(++revision);
      return HttpResponse.json(catalog);
    }),
  ];
}
