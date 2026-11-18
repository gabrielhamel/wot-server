import { useEffect, useState } from 'react';
import './shop.css';

const token = /^#([A-Za-z0-9_-]{43})(?=$|[/?&])/.exec(window.location.hash)?.[1];
const number = new Intl.NumberFormat('en');

async function request(endpoint, order, signal) {
  const headers = { Authorization: `Bearer ${token}` };
  if (order) headers['Content-Type'] = 'application/json';
  const response = await fetch(`/api/shop/${endpoint}`, {
    method: order ? 'POST' : 'GET',
    headers,
    body: order ? JSON.stringify(order) : undefined,
    cache: 'no-store',
    redirect: 'error',
    signal,
  });
  if (!response.ok) {
    throw new Error(await response.text() || 'The Store is temporarily unavailable.');
  }
  return response.json();
}

export default function Shop() {
  const [catalog, setCatalog] = useState(null);
  const [pending, setPending] = useState(Boolean(token));
  const [error, setError] = useState(false);
  const [status, setStatus] = useState(token ? 'Loading offers…' : 'Open the Store from a connected garage.');

  async function load(signal) {
    setPending(true);
    setError(false);
    setStatus('Loading offers…');
    try {
      const result = await request('catalog', undefined, signal);
      setCatalog(result);
      setStatus(result.offers.length ? '' : 'No equipment offers are available.');
    } catch (error) {
      if (signal?.aborted) return;
      setStatus(error.message);
      setError(true);
    } finally {
      if (!signal?.aborted) setPending(false);
    }
  }

  useEffect(() => {
    if (!token) return;
    const controller = new AbortController();
    load(controller.signal);
    return () => controller.abort();
  }, []);

  async function buy(offer) {
    setPending(true);
    setStatus(`Purchasing ${offer.name}…`);
    try {
      const result = await request('orders', {
        item: offer.item,
        revision: catalog.revision,
        credits: offer.credits,
      });
      setCatalog(result);
      setStatus(`${offer.name} added to your Depot.`);
    } catch (error) {
      setStatus(error.message);
      setError(true);
    } finally {
      setPending(false);
    }
  }

  return (
    <main>
      <header>
        <div>
          <h1>Equipment</h1>
          <p>Purchased equipment goes to your Depot.</p>
        </div>
        {catalog && <p id="balance">{number.format(catalog.credits)} credits available</p>}
      </header>
      <p id="status" role="status">{status}</p>
      {error && <button type="button" onClick={() => load()}>Reload offers</button>}
      <ul aria-label="Equipment offers">
        {catalog?.offers.map(offer => (
          <li key={offer.item}>
            <h2>{offer.name}</h2>
            <p>{number.format(offer.in_depot)} in Depot</p>
            <button
              type="button"
              disabled={pending || error || catalog.credits < offer.credits}
              onClick={() => buy(offer)}
            >
              Buy for {number.format(offer.credits)} credits
            </button>
          </li>
        ))}
      </ul>
    </main>
  );
}
