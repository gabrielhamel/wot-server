import type { Context } from "@opentelemetry/api";
import { request } from "../../platform/infrastructure/web/http";
import { webOperation } from "../../platform/infrastructure/web/telemetry";
import type { StoreCatalog, StoreOrder } from "../application/store-contracts";

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

export function placeOrder(token: string, order: StoreOrder) {
  return webOperation(
    owner,
    "web.store.purchase_equipment",
    {
      "input.item": order.item,
      "input.revision": order.revision,
      "input.price.credits": order.credits,
    },
    async (parent, span) => {
      const catalog = await storeRequest(parent, token, "orders", order);
      span.setAttribute("output.offer_count", catalog.offers.length);
      return catalog;
    },
  );
}
