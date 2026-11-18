import type { StoreOffer, StorePrice } from "../application/store-contracts";

type StoreBrowser = Window & {
  jsHostQuery?: (query: { request: string }) => void;
};

const notificationTypes: Record<StorePrice["currency"], string> = {
  credits: "PurchaseForCredits",
  gold: "PurchaseForGold",
  bonds: "PurchaseForCrystal",
};

export function notifyStorePurchase(offer: StoreOffer) {
  const browser: StoreBrowser = window;
  if (typeof browser.jsHostQuery !== "function") return "bridge_unavailable";

  const name = offer.name.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
  const message =
    offer.category === "premium_account"
      ? `WoT Premium Account time added (${name}).`
      : `${name} added to your ${offer.tier === undefined ? "Depot" : "Garage"}.`;
  browser.jsHostQuery({
    request: JSON.stringify({
      command: "notification",
      params: { type: notificationTypes[offer.price.currency], message },
    }),
  });
  return "submitted";
}
