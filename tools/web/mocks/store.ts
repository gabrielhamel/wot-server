import { http, HttpResponse } from "msw";
import type {
  StoreCatalog,
  StoreOffer,
  StoreOrder,
  StorePrice,
} from "../../../src/player_sessions/application/store-contracts";

export const demoStoreToken = "demo".padEnd(43, "_");

const demoOffers: (Omit<StoreOffer, "availability" | "in_depot"> & { locked?: boolean })[] = [
  { item: -1, name: "1 day", category: "premium_account", price: { currency: "gold", amount: 250 } },
  { item: -30, name: "30 days", category: "premium_account", price: { currency: "gold", amount: 2500 } },
  { item: 193, name: "IS-6", category: "premium_vehicles", nation: "ussr", tier: 8, price: { currency: "gold", amount: 9100 } },
  { item: 321, name: "Renault FT", category: "collectors_vehicles", nation: "france", tier: 1, price: { currency: "credits", amount: 0 } },
  { item: 322, name: "KV-85", category: "collectors_vehicles", nation: "ussr", tier: 6, price: { currency: "credits", amount: 619_000 }, locked: true },
  { item: 131, name: "Moon Viewing", category: "styles2d", price: { currency: "bonds", amount: 450 } },
  { item: 132, name: "Montu (IS-3)", category: "styles3d", price: { currency: "bonds", amount: 2000 } },
  { item: 323, name: "Oliver Murdoc", category: "crew_skins", price: { currency: "gold", amount: 200 } },
  { item: 121, name: "Improved Ventilation Class 3", category: "equipment", price: { currency: "credits", amount: 50_000 } },
  { item: 122, name: "Improved Ventilation Class 2", category: "equipment", price: { currency: "credits", amount: 200_000 } },
  { item: 123, name: "Turbocharger Class 1", category: "equipment", price: { currency: "credits", amount: 600_000 } },
  { item: 124, name: "Small Repair Kit", category: "consumables", price: { currency: "credits", amount: 3000 } },
  { item: 125, name: "Steady Hand", category: "crew_directives", price: { currency: "credits", amount: 10_000 } },
  { item: 126, name: "SU-130PM", category: "bond_vehicles", nation: "ussr", tier: 8, price: { currency: "bonds", amount: 10_000 } },
  { item: 127, name: "Venting System", category: "improved_equipment", price: { currency: "bonds", amount: 5000 } },
  { item: 128, name: "Optical Calibration", category: "equipment_directives", price: { currency: "bonds", amount: 8 } },
];

export function storeHandlers() {
  let revision = 1;
  let premiumExpiry = 0;
  const balances = { credits: 900_000, gold: 5000, bonds: 10_000 } satisfies Record<StorePrice["currency"], number>;
  const stock = new Map([[122, 1]]);
  const garage = new Set<number>();

  function catalog(): StoreCatalog {
    return {
      revision: String(revision),
      premium_expires_at: premiumExpiry > Date.now() / 1_000 ? premiumExpiry : null,
      offers: demoOffers.map(({ locked, ...offer }) => {
        let availability: StoreOffer["availability"] = "available";
        if (garage.has(offer.item)) availability = "in_garage";
        else if (locked) availability = "research_required";
        else if (balances[offer.price.currency] < offer.price.amount) availability = "insufficient_funds";
        return { ...offer, availability, in_depot: stock.get(offer.item) ?? 0 };
      }),
    };
  }

  return [
    http.all("/api/shop/*", ({ request }) => {
      if (request.headers.get("Authorization") !== `Bearer ${demoStoreToken}`) {
        return HttpResponse.text("Open the Store from a connected garage.", { status: 401 });
      }
    }),
    http.get("/api/shop/catalog", () => HttpResponse.json(catalog())),
    http.post("/api/shop/orders", async ({ request }) => {
      const order = (await request.json()) as StoreOrder;
      const offer = catalog().offers.find(({ item }) => item === order.item);
      if (!offer) return HttpResponse.text("This offer is unavailable.", { status: 404 });
      const { currency, amount } = offer.price;
      if (order.revision !== String(revision) || order.price?.currency !== currency || order.price.amount !== amount) {
        return HttpResponse.text("The offer or account changed. Review the current price and try again.", { status: 409 });
      }
      if (offer.availability !== "available") {
        return HttpResponse.text("This offer could not be purchased.", { status: 422 });
      }
      balances[currency] -= amount;
      if (offer.category === "premium_account") {
        premiumExpiry = Math.max(premiumExpiry, Math.floor(Date.now() / 1000)) - offer.item * 86_400;
      } else if (offer.tier !== undefined) {
        garage.add(offer.item);
      } else {
        stock.set(offer.item, offer.in_depot + 1);
      }
      revision += 1;
      return HttpResponse.json(catalog());
    }),
  ];
}
