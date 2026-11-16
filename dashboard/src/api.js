import { HttpError } from 'react-admin';

const tokenKey = 'wot-server.admin-token';

async function request(path, { token = sessionStorage.getItem(tokenKey), body, signal } = {}) {
  const headers = new Headers({ Accept: 'application/json' });
  if (token) headers.set('Authorization', `Bearer ${token}`);
  if (body) headers.set('Content-Type', 'application/json');

  const response = await fetch(`/api/admin${path}`, {
    method: body ? 'POST' : 'GET',
    headers,
    body: body ? JSON.stringify(body) : undefined,
    signal,
  });
  if (response.status === 204) return;
  const result = await response.json().catch(() => null);
  if (!response.ok || result === null) {
    throw new HttpError(result?.message ?? 'The server returned an unexpected response.', response.status, result);
  }
  return result;
}

export const authProvider = {
  async login({ token }) {
    const value = token.trim();
    await request('/session', { token: value });
    sessionStorage.setItem(tokenKey, value);
  },
  async logout() {
    sessionStorage.removeItem(tokenKey);
  },
  async checkAuth() {
    if (!sessionStorage.getItem(tokenKey)) throw new Error();
  },
  async checkError(error) {
    if (error.status === 401) {
      sessionStorage.removeItem(tokenKey);
      throw error;
    }
  },
};

export const dataProvider = {
  supportAbortSignal: true,
  getList(_resource, { pagination: { page, perPage }, signal }) {
    const query = new URLSearchParams({ page, per_page: perPage });
    return request(`/players?${query}`, { signal });
  },
  async create(_resource, { data }) {
    return { data: await request('/players', { body: data }) };
  },
  getGarage(id, signal) {
    return request(`/players/${encodeURIComponent(id)}/garage`, { signal });
  },
  getVehicleCatalog(signal) {
    return request('/vehicles', { signal });
  },
  grant(id, body) {
    return request(`/players/${encodeURIComponent(id)}/grants`, { body });
  },
};
