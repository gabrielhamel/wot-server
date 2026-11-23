import { http, HttpResponse } from "msw";
import type { Garage } from "../../../player-administration/src/web";
import type {
  StoreCatalog,
  StoreOffer,
  StoreOrder,
  StorePrice,
} from "../../../player-sessions/src/web";

export const demoStoreToken = "demo".padEnd(43, "_");

const demoOffers: (Omit<StoreOffer, "availability" | "in_depot"> & { locked?: boolean })[] = [
  {
    item: -1,
    name: "1 day",
    category: "premium_account",
    price: { currency: "gold", amount: 250 },
  },
  {
    item: -30,
    name: "30 days",
    category: "premium_account",
    price: { currency: "gold", amount: 2500 },
  },
  {
    item: 193,
    name: "IS-6",
    category: "premium_vehicles",
    nation: "ussr",
    tier: 8,
    price: { currency: "gold", amount: 9100 },
  },
  {
    item: 321,
    name: "Renault FT",
    category: "collectors_vehicles",
    nation: "france",
    tier: 1,
    price: { currency: "credits", amount: 0 },
  },
  {
    item: 322,
    name: "KV-85",
    category: "collectors_vehicles",
    nation: "ussr",
    tier: 6,
    price: { currency: "credits", amount: 619_000 },
    locked: true,
  },
  {
    item: 131,
    name: "Moon Viewing",
    category: "styles2d",
    price: { currency: "bonds", amount: 450 },
  },
  {
    item: 132,
    name: "Montu (IS-3)",
    category: "styles3d",
    price: { currency: "bonds", amount: 2000 },
  },
  {
    item: 323,
    name: "Oliver Murdoc",
    category: "crew_skins",
    price: { currency: "gold", amount: 200 },
  },
  {
    item: 121,
    name: "Improved Ventilation Class 3",
    category: "equipment",
    price: { currency: "credits", amount: 50_000 },
  },
  {
    item: 122,
    name: "Improved Ventilation Class 2",
    category: "equipment",
    price: { currency: "credits", amount: 200_000 },
  },
  {
    item: 123,
    name: "Turbocharger Class 1",
    category: "equipment",
    price: { currency: "credits", amount: 600_000 },
  },
  {
    item: 124,
    name: "Small Repair Kit",
    category: "consumables",
    price: { currency: "credits", amount: 3000 },
  },
  {
    item: 125,
    name: "Steady Hand",
    category: "crew_directives",
    price: { currency: "credits", amount: 10_000 },
  },
  {
    item: 126,
    name: "SU-100Y",
    category: "bond_vehicles",
    nation: "ussr",
    tier: 6,
    price: { currency: "bonds", amount: 5000 },
  },
  {
    item: 126,
    name: "SU-100Y",
    category: "premium_vehicles",
    nation: "ussr",
    tier: 6,
    price: { currency: "gold", amount: 2900 },
  },
  {
    item: 127,
    name: "Venting System",
    category: "improved_equipment",
    price: { currency: "bonds", amount: 5000 },
  },
  {
    item: 128,
    name: "Optical Calibration",
    category: "equipment_directives",
    price: { currency: "bonds", amount: 8 },
  },
];

function demoStore(playerGarage?: Garage) {
  let revision = 1;
  let premiumExpiry = 0;
  const balances = { credits: 900_000, gold: 5000, bonds: 10_000 } satisfies Record<
    StorePrice["currency"],
    number
  >;
  const stock = new Map([[122, 1]]);
  const purchasedVehicles = new Set<number>();
  const balance = (currency: StorePrice["currency"]) =>
    playerGarage ? Number(playerGarage[currency]) : balances[currency];

  function catalog(): StoreCatalog {
    return {
      revision: String(revision),
      premium_expires_at: premiumExpiry > Date.now() / 1_000 ? premiumExpiry : null,
      offers: demoOffers.map(({ locked, ...offer }) => {
        let availability: StoreOffer["availability"] = "available";
        if (purchasedVehicles.has(offer.item)) availability = "in_garage";
        else if (locked) availability = "research_required";
        else if (balance(offer.price.currency) < offer.price.amount)
          availability = "insufficient_funds";
        return { ...offer, availability, in_depot: stock.get(offer.item) ?? 0 };
      }),
    };
  }

  function purchase(order: StoreOrder) {
    const offers = catalog().offers.filter(({ item }) => item === order.item);
    if (offers.length === 0)
      return HttpResponse.text("This offer is unavailable.", { status: 404 });
    const offer = offers.find(({ price }) => price.currency === order.price?.currency);
    if (
      !offer ||
      order.revision !== String(revision) ||
      order.price.amount !== offer.price.amount
    ) {
      return HttpResponse.text(
        "The offer or account changed. Review the current price and try again.",
        { status: 409 },
      );
    }
    if (offer.availability !== "available") {
      return HttpResponse.text("This offer could not be purchased.", { status: 422 });
    }
    const { currency, amount } = offer.price;
    if (playerGarage) {
      playerGarage[currency] = String(balance(currency) - amount);
    } else {
      balances[currency] -= amount;
    }
    if (offer.category === "premium_account") {
      premiumExpiry = Math.max(premiumExpiry, Math.floor(Date.now() / 1000)) - offer.item * 86_400;
    } else if (offer.tier !== undefined) {
      purchasedVehicles.add(offer.item);
      playerGarage?.vehicles.push({
        key: String(offer.item),
        name: offer.name,
        tier: offer.tier,
        inventory_id: offer.item,
        stat_trackers: {},
      });
    } else {
      stock.set(offer.item, offer.in_depot + 1);
    }
    revision += 1;
    return HttpResponse.json(catalog());
  }

  return { catalog, purchase };
}

export function storeHandlers(dashboardGarage?: () => Garage | undefined) {
  const path = dashboardGarage ? "/api/admin/me/shop" : "/api/shop";
  const nativeStore = demoStore();
  const dashboardStores = new WeakMap<Garage, ReturnType<typeof demoStore>>();

  function authorize(request: Request) {
    if (!dashboardGarage) {
      return request.headers.get("Authorization") === `Bearer ${demoStoreToken}`
        ? nativeStore
        : HttpResponse.text("Open the Store from a connected garage.", { status: 401 });
    }
    if (request.method === "POST" && request.headers.get("X-Wot-Admin") !== "1") {
      return HttpResponse.text("The admin request header is required.", { status: 403 });
    }
    const garage = dashboardGarage();
    if (!garage) return HttpResponse.text("Sign in to open the Store.", { status: 401 });
    const store = dashboardStores.get(garage) ?? demoStore(garage);
    dashboardStores.set(garage, store);
    return store;
  }

  return [
    http.get(`${path}/catalog`, ({ request }) => {
      const store = authorize(request);
      return store instanceof Response ? store : HttpResponse.json(store.catalog());
    }),
    http.post(`${path}/orders`, async ({ request }) => {
      const store = authorize(request);
      return store instanceof Response
        ? store
        : store.purchase((await request.json()) as StoreOrder);
    }),
  ];
}
