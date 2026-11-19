import { useEffect, useRef, useState } from "react";
import { errorMessage, responseStatus } from "../../../../platform/infrastructure/web/http";
import type { StoreAccess, StoreCatalog, StoreOffer } from "../../../application/store-contracts";
import { loadCatalog, placeOrder } from "../../../infrastructure/store-api";

export function useStoreCatalog(
  access: StoreAccess,
  onUnauthorized?: () => void,
  onPurchased?: () => void,
) {
  const inFlight = useRef(false);
  const [attempt, setAttempt] = useState(0);
  const [catalog, setCatalog] = useState<StoreCatalog | null>(null);
  const [loading, setLoading] = useState(true);
  const [buyingOffer, setBuyingOffer] = useState<StoreOffer | null>(null);
  const [purchasedOffer, setPurchasedOffer] = useState<StoreOffer | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    const { signal } = controller;
    void loadCatalog(access, signal)
      .then(
        (catalog) => {
          if (!signal.aborted) setCatalog(catalog);
        },
        (error) => {
          if (signal.aborted) return;
          if (responseStatus(error) === 401) onUnauthorized?.();
          setError(errorMessage(error));
        },
      )
      .finally(() => {
        if (!signal.aborted) setLoading(false);
      });
    return () => controller.abort();
  }, [access, attempt, onUnauthorized]);

  function reload() {
    setLoading(true);
    setError(null);
    setAttempt((attempt) => attempt + 1);
  }

  async function buy(offer: StoreOffer) {
    if (!catalog || inFlight.current) return;
    inFlight.current = true;
    setBuyingOffer(offer);
    setPurchasedOffer(null);
    setError(null);
    try {
      setCatalog(await placeOrder(access, offer, catalog.revision));
      setPurchasedOffer(offer);
      onPurchased?.();
    } catch (error) {
      if (responseStatus(error) === 401) onUnauthorized?.();
      setError(errorMessage(error));
    } finally {
      inFlight.current = false;
      setBuyingOffer(null);
    }
  }

  return { catalog, loading, buyingOffer, purchasedOffer, error, reload, buy };
}
