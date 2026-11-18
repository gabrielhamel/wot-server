import type { Context } from "@opentelemetry/api";
import { request } from "../../platform/infrastructure/web/http";
import { webOperation } from "../../platform/infrastructure/web/telemetry";
import type { StoreCatalog, StoreOffer, StoreOrder } from "../application/store-contracts";

import { notifyStorePurchase } from "./store-notifications";

const minimumPurchaseMs = 350;
const owner = "player_sessions";

function storeRequest(
  parent: Context,
  token: string,
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

export function loadCatalog(token: string, signal?: AbortSignal) {
  return webOperation(owner, "web.store.load_catalog", {}, async (parent, span) => {
    const catalog = await storeRequest(parent, token, "catalog", undefined, signal);
    span.setAttribute("output.offer_count", catalog.offers.length);
    return catalog;
  });
}

export function placeOrder(token: string, offer: StoreOffer, revision: string) {
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
      const catalog = await storeRequest(parent, token, "orders", order).finally(() => minimum);
      span.setAttribute("output.offer_count", catalog.offers.length);
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
