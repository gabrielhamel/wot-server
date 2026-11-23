import type { Context, Attributes } from "@opentelemetry/api";
import { defaultDataProvider } from "react-admin";
import type { AuthProvider, DataProvider, Identifier, RaRecord } from "react-admin";
import { request, responseStatus } from "../../../wot-platform/src/infrastructure/web/http";
import { webOperation } from "../../../wot-platform/src/infrastructure/web/telemetry";
import type { StoreCatalog, StoreTransport } from "../../../wot-player-sessions/src/web";
import type {
  AdminSession,
  CatalogVehicle,
  Cosmetic,
  DepotItem,
  DepotGrantKind,
  Garage,
  Grant,
  GrantResult,
  NewPlayer,
  PlayerGarage,
  Registration,
  PlayerRecord,
  PlayerRole,
  PlayerCredentials,
} from "../application/admin-contracts";

const owner = "player_administration";

function adminRequest<T>(
  parent: Context,
  endpoint: string,
  options: {
    path?: string;
    body?: object;
    signal?: AbortSignal;
    errorFormat?: "json" | "text";
  } = {},
) {
  return request<T>({
    owner,
    parent,
    route: `/api/admin${endpoint}`,
    path: `/api/admin${options.path ?? endpoint}`,
    method: options.body ? "POST" : "GET",
    headers: options.body ? { "X-Wot-Admin": "1" } : undefined,
    body: options.body,
    signal: options.signal,
    errorFormat: options.errorFormat ?? "json",
  });
}

export const dashboardStoreTransport: StoreTransport = {
  loadCatalog: (parent, signal) =>
    adminRequest<StoreCatalog>(parent, "/me/shop/catalog", { signal, errorFormat: "text" }),
  submitOrder: (parent, body) =>
    adminRequest<StoreCatalog>(parent, "/me/shop/orders", { body, errorFormat: "text" }),
};

export function getSession(signal?: AbortSignal) {
  return webOperation(owner, "web.admin.session_status", {}, async (parent, span) => {
    const session = await adminRequest<AdminSession>(parent, "/session", { signal });
    span.setAttribute("output.session.status", session.status);
    return session;
  });
}

export function createFirstAdmin(body: NewPlayer) {
  const encoder = new TextEncoder();
  return webOperation(
    owner,
    "web.admin.bootstrap",
    {
      "input.role": "admin",
      "input.email.bytes": encoder.encode(body.username).length,
      "input.display_name.bytes": encoder.encode(body.display_name).length,
    },
    (parent) => adminRequest<PlayerRecord>(parent, "/setup", { body }),
  );
}

export function checkUsername(username: string, signal: AbortSignal) {
  return webOperation(
    owner,
    "web.panel.check_username",
    { "input.username.bytes": username.length },
    async (parent, span) => {
      const { available } = await adminRequest<{ available: boolean }>(
        parent,
        "/registration/username",
        { body: { username }, signal },
      );
      span.setAttribute("output.username.available", available);
      return available;
    },
  );
}

export function registerPlayer(body: Registration) {
  const encoder = new TextEncoder();
  return webOperation(
    owner,
    "web.panel.register",
    {
      "input.username.bytes": encoder.encode(body.username).length,
      "input.email.bytes": encoder.encode(body.email).length,
    },
    (parent) => adminRequest<PlayerRecord>(parent, "/registration", { body }),
  );
}

export function createManagedPlayer({ email, username, password }: PlayerCredentials) {
  const encoder = new TextEncoder();
  return webOperation(
    owner,
    "web.admin.create_player",
    {
      "input.email.bytes": encoder.encode(email).length,
      "input.username.bytes": encoder.encode(username).length,
    },
    (parent) =>
      adminRequest<PlayerRecord>(parent, "/players", {
        body: { username: email, display_name: username, password },
      }),
  );
}

export function getOwnGarage(signal: AbortSignal) {
  return webOperation(owner, "web.panel.load_own_garage", {}, async (parent, span) => {
    const garage = await adminRequest<PlayerGarage>(parent, "/me/garage", { signal });
    span.setAttribute("output.vehicle_count", garage.vehicles.length);
    return garage;
  });
}

function changePlayerRole(id: Identifier, role: PlayerRole) {
  return webOperation(
    owner,
    "web.admin.change_role",
    { "input.account_id": inputAccountId(id), "input.role": role },
    (parent) =>
      adminRequest<void>(parent, "/players/{id}/role", {
        path: `/players/${encodeURIComponent(id)}/role`,
        body: { role },
      }),
  );
}

export const authProvider: AuthProvider = {
  async login({ email, password }: { email: string; password: string }) {
    await webOperation(owner, "web.admin.sign_in", {}, (parent) =>
      adminRequest<void>(parent, "/session", { body: { email, password } }),
    );
  },
  async logout(params?: { revokeSession?: boolean }) {
    if (!params?.revokeSession) return;
    await webOperation(owner, "web.admin.sign_out", {}, (parent) =>
      adminRequest<void>(parent, "/logout", { body: {} }),
    );
  },
  async checkAuth() {
    if ((await getSession()).status !== "authenticated") throw new Error();
  },
  async getIdentity() {
    const session = await getSession();
    if (session.status !== "authenticated") throw new Error();
    return {
      id: session.player.id,
      fullName: session.player.display_name,
      role: session.player.role,
      storeEnabled: session.store_enabled,
    };
  },
  async checkError(error: unknown) {
    if (responseStatus(error) === 401) throw error;
  },
};

function grantInputs(grant: Grant): Attributes {
  switch (grant.kind) {
    case "currency":
      return { "input.grant.currency": grant.currency, "input.amount": grant.amount };
    case "vehicle":
      return { "input.vehicle_type": grant.vehicle };
    case "crew_book":
    case "experimental_equipment":
    case "improved_equipment":
    case "bounty_equipment":
      return { "input.item": grant.id, "input.count": grant.count };
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

const depotCatalogs = {
  crew_book: "crew-books",
  experimental_equipment: "experimental-equipment",
  bounty_equipment: "bounty-equipment",
  improved_equipment: "improved-equipment",
} as const satisfies Record<DepotGrantKind, string>;

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
  getDepotCatalog(kind: DepotGrantKind, signal?: AbortSignal) {
    return loadCatalog<DepotItem>(depotCatalogs[kind], signal);
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
  catalog: "vehicles" | "attachments" | "stat-trackers" | (typeof depotCatalogs)[DepotGrantKind],
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

export type AdminDataProvider = DataProvider &
  typeof garageApi & { changePlayerRole: typeof changePlayerRole };

export const dataProvider: AdminDataProvider = {
  ...defaultDataProvider,
  ...garageApi,
  changePlayerRole,
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
};
