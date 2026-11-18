import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../../../../platform/infrastructure/web/http";
import type { StoreCatalog, StoreOffer } from "../../../application/store-contracts";
import { loadCatalog, placeOrder } from "../../../infrastructure/store-api";

export function useEquipmentCatalog(token?: string) {
  const inFlight = useRef(false);
  const [catalog, setCatalog] = useState<StoreCatalog | null>(null);
  const [loading, setLoading] = useState(Boolean(token));
  const [buyingItem, setBuyingItem] = useState<number | null>(null);
  const [feedback, setFeedback] = useState<{
    severity: "success" | "error";
    message: string;
  } | null>(null);

  const load = useCallback(
    (signal?: AbortSignal) => {
      if (!token) return;
      void loadCatalog(token, signal)
        .then(
          (catalog) => {
            if (!signal?.aborted) setCatalog(catalog);
          },
          (error) => {
            if (!signal?.aborted) setFeedback({ severity: "error", message: errorMessage(error) });
          },
        )
        .finally(() => {
          if (!signal?.aborted) setLoading(false);
        });
    },
    [token],
  );

  useEffect(() => {
    if (!token) return;
    const controller = new AbortController();
    load(controller.signal);
    return () => controller.abort();
  }, [load, token]);

  function reload() {
    setLoading(true);
    setFeedback(null);
    load();
  }

  async function buy(offer: StoreOffer) {
    if (!catalog || !token || inFlight.current) return;
    inFlight.current = true;
    setBuyingItem(offer.item);
    setFeedback(null);
    try {
      const result = await placeOrder(token, {
        item: offer.item,
        revision: catalog.revision,
        credits: offer.credits,
      });
      setCatalog(result);
      setFeedback({ severity: "success", message: `${offer.name} added to your Depot.` });
    } catch (error) {
      setFeedback({ severity: "error", message: errorMessage(error) });
    } finally {
      inFlight.current = false;
      setBuyingItem(null);
    }
  }

  return { catalog, loading, buyingItem, feedback, reload, buy };
}
