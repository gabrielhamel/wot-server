import type {
  AdminBattleSetup,
  BattleOptions,
  CustomBattleLoadout,
} from "../../application/admin-contracts";
import { getBattleLoadout } from "../../infrastructure/admin-api";

export const lastBattleKey = "wot.admin.last-battle.v1";
export type LastBattle = Omit<AdminBattleSetup, "players" | "parameters"> & {
  parameters: Record<string, number>;
};

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function integer(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function numbers(value: unknown): value is number[] {
  return Array.isArray(value) && value.every(integer);
}

function loadout(value: unknown): value is CustomBattleLoadout {
  return (
    record(value) &&
    integer(value.vehicle) &&
    (value.equipment == null || numbers(value.equipment)) &&
    (value.consumables == null || numbers(value.consumables)) &&
    (value.hit_points === undefined || integer(value.hit_points)) &&
    (value.modules === undefined ||
      (record(value.modules) && Object.values(value.modules).every(integer))) &&
    (value.crew_perks === undefined ||
      (Array.isArray(value.crew_perks) &&
        value.crew_perks.every((seat) => seat === null || numbers(seat)))) &&
    (value.crew_perk_efficiency === undefined ||
      (Array.isArray(value.crew_perk_efficiency) &&
        value.crew_perk_efficiency.every((seat) => seat === null || integer(seat)))) &&
    (value.ammunition === undefined ||
      (Array.isArray(value.ammunition) &&
        value.ammunition.every((pair) => numbers(pair) && pair.length === 2)))
  );
}

function read(): LastBattle | undefined {
  try {
    const raw = localStorage.getItem(lastBattleKey);
    if (!raw) return;
    const data = JSON.parse(raw) as unknown;
    if (
      !record(data) ||
      !integer(data.arena_type) ||
      !integer(data.duration_seconds) ||
      !record(data.parameters) ||
      !Object.values(data.parameters).every(Number.isFinite) ||
      !Array.isArray(data.dummies) ||
      data.dummies.length >= 30 ||
      !data.dummies.every(
        (dummy) =>
          record(dummy) && (dummy.team === 1 || dummy.team === 2) && loadout(dummy.loadout),
      )
    )
      return;
    return data as LastBattle;
  } catch {
    return undefined;
  }
}

export function saveLastBattle(setup: LastBattle) {
  try {
    localStorage.setItem(lastBattleKey, JSON.stringify(setup));
    return true;
  } catch {
    return false;
  }
}

export async function restoreLastBattle(options: BattleOptions, signal: AbortSignal) {
  const saved = read();
  if (!saved) return;
  const warnings: string[] = [];
  const map = options.maps.some((choice) => choice.id === saved.arena_type)
    ? saved.arena_type
    : ("" as const);
  if (map === "") warnings.push("Select a map. The saved map is unavailable.");
  const parameters: Record<string, string> = {};
  for (const definition of options.parameters) {
    const value = saved.parameters[definition.key];
    if (value === undefined) continue;
    if (
      value < definition.min ||
      value > definition.max ||
      (definition.integer && !Number.isInteger(value))
    ) {
      warnings.push(`Review ${definition.label}. Its saved value is outside the current range.`);
    } else {
      parameters[definition.key] = String(value);
    }
  }
  const catalogs = new Map<number, ReturnType<typeof getBattleLoadout>>();
  const dummies = await Promise.all(
    saved.dummies.map(async ({ team, loadout }, index) => {
      const name = `Dummy ${index + 1}`;
      if (!options.dummy_vehicles.some((choice) => choice.id === loadout.vehicle)) {
        warnings.push(`${name}: select a vehicle. The saved vehicle is unavailable.`);
        return { team, loadout: { vehicle: 0 } };
      }
      if (loadout.equipment?.some(Boolean) || loadout.consumables?.some(Boolean)) {
        let request = catalogs.get(loadout.vehicle);
        if (!request) {
          request = getBattleLoadout(loadout.vehicle, signal);
          catalogs.set(loadout.vehicle, request);
        }
        try {
          const catalog = await request;
          for (const kind of ["equipment", "consumables"] as const) {
            const items = loadout[kind];
            if (!items) continue;
            const slots = kind === "equipment" ? catalog.equipment_slots : catalog.consumable_slots;
            const current = items
              .slice(0, slots)
              .map((id) => (catalog[kind].some((choice) => choice.id === id) ? id : 0));
            if (items.some((id, i) => id !== 0 && id !== current[i])) {
              warnings.push(`${name}: review ${kind}. A saved selection is unavailable.`);
            }
            loadout[kind] = current;
          }
        } catch {
          warnings.push(`${name}: could not verify saved equipment and consumables.`);
        }
      }
      return { team, loadout };
    }),
  );
  return { map, duration: String(saved.duration_seconds / 60), parameters, dummies, warnings };
}
