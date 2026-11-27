import type { BattleModuleKind, CustomBattleLoadout } from "../../application/admin-contracts";

export const MAX_ENTRANTS = 30;

const DEFAULT_DURATION = "15";
const PARAMETER_PREFIX = "param.";

export type Player = {
  account_id: number;
  team: number;
  vehicle: number | "";
  vehicleSource: "owned" | "custom";
  customLoadout: CustomBattleLoadout;
  ownedEdits: Record<number, CustomBattleLoadout>;
};

export type Dummy = { key: number; team: number; loadout: CustomBattleLoadout };

export type BattleDraft = {
  map: number | "";
  duration: string;
  parameterDrafts: Record<string, string>;
  players: Player[];
  dummies: Dummy[];
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value);
}

function isTeam(value: unknown) {
  return value === 1 || value === 2;
}

function isList(value: unknown, isItem: (item: unknown) => boolean) {
  return Array.isArray(value) && value.every(isItem);
}

const moduleKinds = {
  Chassis: true,
  Turret: true,
  Gun: true,
  Engine: true,
  FuelTank: true,
  Radio: true,
} satisfies Record<BattleModuleKind, true>;

const loadoutFields: Record<keyof CustomBattleLoadout, (value: unknown) => boolean> = {
  vehicle: isInteger,
  equipment: (value) => value === null || isList(value, isInteger),
  consumables: (value) => value === null || isList(value, isInteger),
  crew_perks: (value) => isList(value, (perks) => perks === null || isList(perks, isInteger)),
  crew_perk_efficiency: (value) =>
    isList(value, (percent) => percent === null || isInteger(percent)),
  hit_points: isInteger,
  ammunition: (value) =>
    isList(value, (pair) => Array.isArray(pair) && pair.length === 2 && pair.every(isInteger)),
  modules: (value) =>
    isRecord(value) &&
    Object.entries(value).every(([kind, id]) => Object.hasOwn(moduleKinds, kind) && isInteger(id)),
};

function isLoadout(value: unknown): value is CustomBattleLoadout {
  return (
    isRecord(value) &&
    isInteger(value.vehicle) &&
    Object.entries(value).every(
      ([key, field]) =>
        Object.hasOwn(loadoutFields, key) && loadoutFields[key as keyof CustomBattleLoadout](field),
    )
  );
}

function isPlayer(value: unknown): value is Player {
  return (
    isRecord(value) &&
    isInteger(value.account_id) &&
    isTeam(value.team) &&
    (value.vehicle === "" || isInteger(value.vehicle)) &&
    (value.vehicleSource === "owned" || value.vehicleSource === "custom") &&
    isLoadout(value.customLoadout) &&
    isRecord(value.ownedEdits) &&
    Object.entries(value.ownedEdits).every(
      ([inventory, loadout]) => /^-?\d+$/.test(inventory) && isLoadout(loadout),
    )
  );
}

function isDummy(value: unknown): value is Dummy {
  return isRecord(value) && isInteger(value.key) && isTeam(value.team) && isLoadout(value.loadout);
}

function parseEntry(value: string): unknown {
  try {
    return JSON.parse(value);
  } catch {
    return undefined;
  }
}

function numericInput(value: string) {
  return /^-?(?:\d+(?:\.\d+)?|\.\d+)(?:e[+-]?\d+)?$/i.test(value) && Number.isFinite(Number(value))
    ? value
    : "";
}

export function readBattleDraft(search: string): BattleDraft {
  const query = new URLSearchParams(search);
  const map = query.get("map");
  const players: Player[] = [];
  const dummies: Dummy[] = [];
  const parameters: [string, string][] = [];

  for (const [key, value] of query) {
    if (key.startsWith(PARAMETER_PREFIX)) {
      parameters.push([key.slice(PARAMETER_PREFIX.length), numericInput(value)]);
      continue;
    }
    if (key === "player" && players.length < MAX_ENTRANTS) {
      const player = parseEntry(value);
      if (isPlayer(player) && !players.some(({ account_id }) => account_id === player.account_id)) {
        const { account_id, team, vehicle, vehicleSource, customLoadout, ownedEdits } = player;
        players.push({ account_id, team, vehicle, vehicleSource, customLoadout, ownedEdits });
      }
    }
    if (key === "dummy" && dummies.length < MAX_ENTRANTS) {
      const dummy = parseEntry(value);
      if (isDummy(dummy) && !dummies.some(({ key }) => key === dummy.key)) {
        const { key, team, loadout } = dummy;
        dummies.push({ key, team, loadout });
      }
    }
  }

  return {
    map: map && /^-?\d+$/.test(map) && isInteger(Number(map)) ? Number(map) : "",
    duration: numericInput(query.get("duration") ?? DEFAULT_DURATION),
    parameterDrafts: Object.fromEntries(parameters),
    players,
    dummies,
  };
}

export function updateBattleQuery(search: string, draft: Partial<BattleDraft>) {
  const query = new URLSearchParams(search);
  if (draft.map !== undefined) {
    if (draft.map === "") query.delete("map");
    else query.set("map", String(draft.map));
  }
  if (draft.duration !== undefined) {
    if (draft.duration === DEFAULT_DURATION) query.delete("duration");
    else query.set("duration", draft.duration);
  }
  if (draft.parameterDrafts) {
    for (const key of Array.from(query.keys())) {
      if (key.startsWith(PARAMETER_PREFIX)) query.delete(key);
    }
    for (const [key, value] of Object.entries(draft.parameterDrafts)) {
      query.set(`${PARAMETER_PREFIX}${key}`, value);
    }
  }
  if (draft.players) {
    query.delete("player");
    for (const player of draft.players) query.append("player", JSON.stringify(player));
  }
  if (draft.dummies) {
    query.delete("dummy");
    for (const dummy of draft.dummies) query.append("dummy", JSON.stringify(dummy));
  }
  const encoded = query.toString();
  return encoded ? `?${encoded}` : "";
}
