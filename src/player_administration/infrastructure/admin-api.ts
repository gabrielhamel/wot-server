import type { Context, Attributes } from "@opentelemetry/api";
import { defaultDataProvider } from "react-admin";
import type { AuthProvider, CreateParams, DataProvider, Identifier, RaRecord } from "react-admin";
import { request, responseStatus } from "../../platform/infrastructure/web/http";
import { webOperation } from "../../platform/infrastructure/web/telemetry";
import type {
  CatalogVehicle,
  Cosmetic,
  Garage,
  Grant,
  GrantResult,
  NewPlayer,
  PlayerRecord,
} from "../application/admin-contracts";

const tokenKey = "wot-server.admin-token";
const owner = "player_administration";

function adminRequest<T>(
  parent: Context,
  endpoint: string,
  options: { path?: string; token?: string; body?: object; signal?: AbortSignal } = {},
) {
  return request<T>({
    owner,
    parent,
    route: `/api/admin${endpoint}`,
    path: `/api/admin${options.path ?? endpoint}`,
    method: options.body ? "POST" : "GET",
    token: options.token ?? sessionStorage.getItem(tokenKey),
    body: options.body,
    signal: options.signal,
    errorFormat: "json",
  });
}

export const authProvider: AuthProvider = {
  async login({ token }: { token: string }) {
    const value = token.trim();
    await webOperation(owner, "web.admin.sign_in", {}, (parent) =>
      adminRequest<void>(parent, "/session", { token: value }),
    );
    sessionStorage.setItem(tokenKey, value);
  },
  async logout() {
    sessionStorage.removeItem(tokenKey);
  },
  async checkAuth() {
    if (!sessionStorage.getItem(tokenKey)) throw new Error();
  },
  async checkError(error: unknown) {
    if (responseStatus(error) === 401) {
      sessionStorage.removeItem(tokenKey);
      throw error;
    }
  },
};

function grantInputs(grant: Grant): Attributes {
  switch (grant.kind) {
    case "currency":
      return { "input.grant.currency": grant.currency, "input.amount": grant.amount };
    case "vehicle":
      return { "input.vehicle_type": grant.vehicle };
    case "attachment":
      return { "input.attachment_id": grant.id, "input.count": grant.count };
    case "stat_tracker":
      return {
        "input.stat_tracker_id": grant.id,
        "input.count": grant.count,
        "input.inventory_id": grant.vehicle,
      };
  }
}

function inputAccountId(id: Identifier) {
  const accountId = String(id);
  return /^[1-9]\d{0,18}$/.test(accountId) ? accountId : undefined;
}

const garageApi = {
  getGarage(id: Identifier, signal?: AbortSignal) {
    return webOperation(
      owner,
      "web.admin.load_garage",
      { "input.account_id": inputAccountId(id) },
      (parent, span) =>
        adminRequest<Garage>(parent, "/players/{id}/garage", {
          path: `/players/${encodeURIComponent(id)}/garage`,
          signal,
        }).then((garage) => {
          span.setAttribute("output.vehicle_count", garage.vehicles.length);
          return garage;
        }),
    );
  },
  getVehicleCatalog(signal?: AbortSignal) {
    return loadCatalog<CatalogVehicle>("vehicles", signal);
  },
  getAttachmentCatalog(signal?: AbortSignal) {
    return loadCatalog<Cosmetic>("attachments", signal);
  },
  getStatTrackerCatalog(signal?: AbortSignal) {
    return loadCatalog<Cosmetic>("stat-trackers", signal);
  },
  grant(id: Identifier, body: Grant) {
    return webOperation(
      owner,
      "web.admin.grant",
      {
        "input.account_id": inputAccountId(id),
        "input.grant.kind": body.kind,
        ...grantInputs(body),
      },
      async (parent, span) => {
        const result = await adminRequest<GrantResult>(parent, "/players/{id}/grants", {
          path: `/players/${encodeURIComponent(id)}/grants`,
          body,
        });
        span.setAttribute("output.delivery", result.delivery);
        return result;
      },
    );
  },
};

function loadCatalog<T>(
  catalog: "vehicles" | "attachments" | "stat-trackers",
  signal?: AbortSignal,
) {
  return webOperation(
    owner,
    "web.admin.load_grant_catalog",
    { "input.catalog": catalog },
    async (parent, span) => {
      const items = await adminRequest<T[]>(parent, `/${catalog}`, { signal });
      span.setAttribute("output.item_count", items.length);
      return items;
    },
  );
}

export type AdminDataProvider = DataProvider & typeof garageApi;

export const dataProvider: AdminDataProvider = {
  ...defaultDataProvider,
  ...garageApi,
  supportAbortSignal: true,
  async getList<RecordType extends RaRecord = PlayerRecord>(
    _resource: string,
    { pagination, signal }: Parameters<DataProvider["getList"]>[1],
  ) {
    const { page, perPage } = pagination ?? { page: 1, perPage: 25 };
    const query = new URLSearchParams({ page: String(page), per_page: String(perPage) });
    return webOperation(
      owner,
      "web.admin.list_players",
      { "input.page": page, "input.per_page": perPage },
      async (parent, span) => {
        const result = await adminRequest<{ data: RecordType[]; total: number }>(
          parent,
          "/players",
          {
            path: `/players?${query}`,
            signal,
          },
        );
        span.setAttribute("output.player_count", result.data.length);
        return result;
      },
    );
  },
  async create<
    Payload extends Omit<RaRecord, "id">,
    Result extends RaRecord = Payload & { id: Identifier },
  >(_resource: string, { data }: CreateParams<Payload>) {
    const body = {
      username: data.username,
      display_name: data.display_name,
      password: data.password,
    } as NewPlayer;
    return webOperation(owner, "web.admin.create_player", {}, async (parent) => ({
      data: await adminRequest<Result>(parent, "/players", { body }),
    }));
  },
};
