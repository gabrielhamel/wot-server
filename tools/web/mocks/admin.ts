import { http, HttpResponse } from "msw";
import { storeHandlers } from "./store";
import type {
  AdminSession,
  CatalogVehicle,
  Cosmetic,
  Garage,
  Grant,
  GrantResult,
  NewPlayer,
  PlayerRecord,
  PlayerRole,
  Registration,
} from "../../../src/player_administration/application/admin-contracts";

const endpoint = "/api/admin";
const sessionKey = "wot-server.demo-session";
const vehicles: CatalogVehicle[] = [
  { id: 101, key: "usa:A01_T1_Cunningham", name: "T1 Cunningham", tier: 1 },
  { id: 813, key: "germany:G01_Pz_IV", name: "Panzer IV", tier: 5 },
];
const attachments: Cosmetic[] = [{ id: 12, name: "Field crate", max_owned: 3 }];
const trackers: Cosmetic[] = [{ id: 37, name: "Victories", max_owned: 1 }];

function problem(status: number, message: string) {
  return HttpResponse.json({ message }, { status });
}

function demoGarage(displayName: string): Garage {
  const { id, ...starter } = vehicles[0];
  return {
    display_name: displayName,
    credits: "900000",
    gold: "250",
    bonds: "100",
    free_xp: "1500",
    slots: 5,
    vehicles: [{ ...starter, inventory_id: id, stat_trackers: { 37: 0 } }],
    attachments: {},
  };
}

function applyGrant(garage: Garage, grant: Grant) {
  switch (grant.kind) {
    case "currency": {
      const balance = Number(garage[grant.currency]) + grant.amount;
      if (!Number.isSafeInteger(grant.amount) || grant.amount < 1 || balance > 2_147_483_647) {
        return problem(422, "Invalid grant amount.");
      }
      garage[grant.currency] = String(balance);
      break;
    }
    case "vehicle": {
      const vehicle = vehicles.find(({ id }) => id === grant.vehicle);
      if (!vehicle) return problem(404, "Vehicle unavailable.");
      if (garage.vehicles.some(({ key }) => key === vehicle.key)) {
        return problem(409, "The player already owns this vehicle.");
      }
      const { id, ...tank } = vehicle;
      garage.vehicles.push({ ...tank, inventory_id: id, stat_trackers: { 37: 0 } });
      break;
    }
    case "attachment":
    case "stat_tracker": {
      const items = grant.kind === "attachment" ? attachments : trackers;
      const owned =
        grant.kind === "attachment"
          ? garage.attachments
          : garage.vehicles.find(({ inventory_id }) => inventory_id === grant.vehicle)
              ?.stat_trackers;
      const item = items.find(({ id }) => id === grant.id);
      if (!item || !owned) return problem(404, "Item unavailable.");
      const count = (owned[grant.id] ?? 0) + grant.count;
      if (!Number.isSafeInteger(grant.count) || grant.count < 1 || grant.count > 100) {
        return problem(422, "Invalid grant count.");
      }
      if (item.max_owned !== null && count > item.max_owned) {
        return problem(409, "This grant would exceed the ownership limit.");
      }
      owned[grant.id] = count;
      break;
    }
    default:
      return problem(422, "Unknown grant type.");
  }
  return HttpResponse.json<GrantResult>({ garage, delivery: "on_next_sync" });
}

export function adminHandlers(storage: Storage = window.localStorage) {
  const admin: PlayerRecord = {
    id: "1",
    username: "admin@example.test",
    display_name: "Demo Administrator",
    role: "admin",
  };
  const players: PlayerRecord[] = [
    admin,
    { id: "2", username: "commander@example.test", display_name: "Demo Commander", role: "player" },
  ];
  const garages = new Map(players.map((player) => [player.id, demoGarage(player.display_name)]));
  const passwords = new Map([[admin.username, "demo-admin"], [players[1].username, "demo-player"]]);
  const savedPlayer = storage.getItem(sessionKey);
  let signedIn = savedPlayer === null ? admin : players.find(({ id }) => id === savedPlayer);
  const nameTaken = (name: string) =>
    players.some((player) => player.display_name.toLowerCase() === name.toLowerCase());

  return [
    http.get(`${endpoint}/session`, () =>
      HttpResponse.json<AdminSession>(
        signedIn ? { status: "authenticated", player: signedIn, store_enabled: true } : { status: "signed_out", registration_open: true },
      ),
    ),
    http.post(`${endpoint}/session`, async ({ request }) => {
      const { email, password } = (await request.json()) as { email: string; password: string };
      if (passwords.get(email) !== password) return problem(401, "Invalid email or password.");
      signedIn = players.find((player) => player.username === email);
      storage.setItem(sessionKey, signedIn?.id ?? "");
      return new HttpResponse(null, { status: 204 });
    }),
    http.post(`${endpoint}/setup`, () => problem(409, "An administrator already exists.")),
    http.post(`${endpoint}/registration/username`, async ({ request }) => {
      const { username } = (await request.json()) as Pick<Registration, "username">;
      return HttpResponse.json({ available: !nameTaken(username) });
    }),
    http.post(`${endpoint}/registration`, async ({ request }) => {
      const { email, username, password } = (await request.json()) as Registration;
      if (players.some((player) => player.username.toLowerCase() === email.toLowerCase())) {
        return problem(409, "That login is already in use.");
      }
      if (nameTaken(username)) return problem(409, "That username is already taken.");
      const player: PlayerRecord = {
        id: String(players.length + 1),
        username: email,
        display_name: username,
        role: "player",
      };
      players.push(player);
      passwords.set(email, password);
      garages.set(player.id, {
        ...demoGarage(username),
        credits: "200000",
        gold: "500",
        bonds: "0",
        free_xp: "0",
      });
      return HttpResponse.json(player, { status: 201 });
    }),
    http.get(`${endpoint}/me/garage`, () => {
      if (!signedIn) return problem(401, "Sign in to view your garage.");
      return HttpResponse.json(garages.get(signedIn.id));
    }),
    http.post(`${endpoint}/logout`, () => {
      signedIn = undefined;
      storage.setItem(sessionKey, "");
      return new HttpResponse(null, { status: 204 });
    }),
    ...storeHandlers(() => (signedIn ? garages.get(signedIn.id) : undefined)),
    http.all(`${endpoint}/*`, () => {
      if (signedIn?.role !== "admin") return problem(401, "Sign in as an administrator.");
    }),
    http.get(`${endpoint}/players`, ({ request }) => {
      const query = new URL(request.url).searchParams;
      const page = Number(query.get("page") ?? 1);
      const perPage = Number(query.get("per_page") ?? 25);
      const start = (page - 1) * perPage;
      return HttpResponse.json({ data: players.slice(start, start + perPage), total: players.length });
    }),
    http.post(`${endpoint}/players`, async ({ request }) => {
      const { username, display_name, password } = (await request.json()) as NewPlayer;
      if (players.some((player) => player.username === username)) {
        return problem(409, "That login is already in use.");
      }
      const player: PlayerRecord = { id: String(players.length + 1), username, display_name, role: "player" };
      if (nameTaken(display_name)) return problem(409, "That username is already taken.");
      players.push(player);
      passwords.set(username, password);
      garages.set(player.id, { ...demoGarage(player.display_name), bonds: "0", free_xp: "0" });
      return HttpResponse.json(player, { status: 201 });
    }),
    http.post(`${endpoint}/players/:id/role`, async ({ request, params }) => {
      const player = players.find(({ id }) => id === params.id);
      if (!player) return problem(404, "Player not found.");
      if (player === admin) return problem(403, "You cannot change your own role.");
      const { role } = (await request.json()) as { role: PlayerRole };
      player.role = role;
      return new HttpResponse(null, { status: 204 });
    }),
    http.get(`${endpoint}/players/:id/garage`, ({ params }) => {
      const garage = garages.get(String(params.id));
      return garage ? HttpResponse.json(garage) : problem(404, "Player not found.");
    }),
    http.post(`${endpoint}/players/:id/grants`, async ({ request, params }) => {
      const garage = garages.get(String(params.id));
      if (!garage) return problem(404, "Player not found.");
      return applyGrant(garage, (await request.json()) as Grant);
    }),
    http.get(`${endpoint}/vehicles`, () => HttpResponse.json(vehicles)),
    http.get(`${endpoint}/attachments`, () => HttpResponse.json(attachments)),
    http.get(`${endpoint}/stat-trackers`, () => HttpResponse.json(trackers)),
  ];
}
