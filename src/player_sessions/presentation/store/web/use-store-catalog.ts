import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../../../../platform/infrastructure/web/http";
import type { StoreCatalog, StoreOffer } from "../../../application/store-contracts";
import { loadCatalog, placeOrder } from "../../../infrastructure/store-api";

export function useStoreCatalog(token?: string) {
  const inFlight = useRef(false);
  const [catalog, setCatalog] = useState<StoreCatalog | null>(null);
  const [loading, setLoading] = useState(Boolean(token));
  const [buyingItem, setBuyingItem] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(
    (signal?: AbortSignal) => {
      if (!token) return;
      void loadCatalog(token, signal)
        .then(
          (catalog) => {
            if (!signal?.aborted) setCatalog(catalog);
          },
          (error) => {
            if (!signal?.aborted) setError(errorMessage(error));
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
    setError(null);
    load();
  }

  async function buy(offer: StoreOffer) {
    if (!catalog || !token || inFlight.current) return;
    inFlight.current = true;
    setBuyingItem(offer.item);
    setError(null);
    try {
      setCatalog(await placeOrder(token, offer, catalog.revision));
    } catch (error) {
      setError(errorMessage(error));
    } finally {
      inFlight.current = false;
      setBuyingItem(null);
    }
  }

  return { catalog, loading, buyingItem, error, reload, buy };
}
