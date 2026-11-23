import type { Context } from "@opentelemetry/api";
import { request } from "../../../platform/src/infrastructure/web/http";
import { webOperation } from "../../../platform/src/infrastructure/web/telemetry";
import type {
  StoreCatalog,
  StoreOffer,
  StoreOrder,
  StoreTransport,
} from "../application/store-contracts";

import { notifyStorePurchase } from "./store-notifications";

const minimumPurchaseMs = 350;
const owner = "player_sessions";

export function garageStoreTransport(token: string): StoreTransport {
  function storeRequest(
    parent: Context,
    endpoint: "catalog" | "orders",
    body?: StoreOrder,
    signal?: AbortSignal,
  ) {
    const path = `/api/shop/${endpoint}`;
    return request<StoreCatalog>({
      owner,
      parent,
      route: path,
      path,
      method: body ? "POST" : "GET",
      token,
      body,
      signal,
      errorFormat: "text",
    });
  }

  return {
    loadCatalog: (parent, signal) => storeRequest(parent, "catalog", undefined, signal),
    submitOrder: (parent, order) => storeRequest(parent, "orders", order),
  };
}

export function loadCatalog(transport: StoreTransport, signal?: AbortSignal) {
  return webOperation(owner, "web.store.load_catalog", {}, async (parent, span) => {
    const catalog = await transport.loadCatalog(parent, signal);
    span.setAttribute("output.offer_count", catalog.offers.length);
    return catalog;
  });
}

export function placeOrder(
  transport: StoreTransport,
  offer: StoreOffer,
  revision: string,
  notifyNativeClient: boolean,
) {
  const order: StoreOrder = { item: offer.item, price: offer.price, revision };
  return webOperation(
    owner,
    "web.store.purchase",
    {
      "input.item": order.item,
      "input.revision": order.revision,
      "input.price.currency": order.price.currency,
      "input.price.amount": order.price.amount,
    },
    async (parent, span) => {
      const minimum = new Promise<void>((resolve) => window.setTimeout(resolve, minimumPurchaseMs));
      const catalog = await transport.submitOrder(parent, order).finally(() => minimum);
      span.setAttribute("output.offer_count", catalog.offers.length);
      if (!notifyNativeClient) return catalog;
      try {
        span.setAttribute("output.notification", notifyStorePurchase(offer));
      } catch {
        span.setAttribute("output.notification", "submission_failed");
        span.addEvent("store.notification.failed");
      }
      return catalog;
    },
  );
}
