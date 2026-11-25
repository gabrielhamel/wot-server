import {
  Alert,
  Autocomplete,
  Button,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Slider,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { Fragment, useEffect, useId, useState } from "react";
import type {
  BattleModuleKind,
  CustomBattleLoadout,
  CustomBattleLoadoutOptions,
} from "../../application/admin-contracts";
import { getBattleLoadout, getOwnedBattleLoadout } from "../../infrastructure/admin-api";
import { errorMessage } from "../../../../shared-infrastructure/src/infrastructure/web/http";
import { ChoiceSelect } from "./battle-select";

function replaceSlot(items: number[], slot: number, item: number) {
  const layout = Array.from(
    { length: Math.max(items.length, slot + 1) },
    (_, index) => items[index] ?? 0,
  );
  layout[slot] = item;
  return layout;
}

const maxHitPoints = 32_767;

function validInteger(value: string, min: number, max: number) {
  const number = Number(value);
  return value !== "" && Number.isInteger(number) && number >= min && number <= max;
}

const moduleFields: [BattleModuleKind, string][] = [
  ["Chassis", "Suspension"],
  ["Turret", "Turret"],
  ["Gun", "Gun"],
  ["Engine", "Engine"],
  ["FuelTank", "Fuel tank"],
  ["Radio", "Radio"],
];

export function BattleLoadoutConfiguration({
  name,
  catalogVehicle,
  ownedVehicle,
  initial,
  onClose,
  onSave,
}: {
  name: string;
  catalogVehicle?: number;
  ownedVehicle?: { account: number; inventory: number };
  initial?: CustomBattleLoadout;
  onClose: () => void;
  onSave: (value: CustomBattleLoadout) => void;
}) {
  const [loadout, setLoadout] = useState(initial);
  const [hitPoints, setHitPoints] = useState(initial?.hit_points?.toString());
  const [ammoEdits, setAmmoEdits] = useState<Record<number, string>>({});
  const [crewEdits, setCrewEdits] = useState<Record<number, number>>({});
  const crewLabelId = useId();
  const [options, setOptions] = useState<CustomBattleLoadoutOptions>();
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const account = ownedVehicle?.account;
  const inventory = ownedVehicle?.inventory;
  useEffect(() => {
    const controller = new AbortController();
    const request =
      account !== undefined && inventory !== undefined
        ? getOwnedBattleLoadout(account, inventory, controller.signal)
        : getBattleLoadout(catalogVehicle ?? 0, controller.signal);
    request.then(
      (result) => {
        if (!controller.signal.aborted) setOptions(result);
      },
      (failure) => {
        if (!controller.signal.aborted)
          setError(errorMessage(failure, "Could not load battle vehicle customization."));
      },
    );
    return () => controller.abort();
  }, [attempt, account, catalogVehicle, inventory]);

  const current = options?.current;
  const vehicle = catalogVehicle ?? current?.vehicle;
  const baseModules = { ...options?.stock_modules, ...current?.modules };
  const modules = { ...baseModules, ...loadout?.modules };
  const weapons = options?.weapons ?? [];
  const weapon = weapons.find(
    ({ turret, gun }) => turret === modules.Turret && gun === modules.Gun,
  );
  const gunIds = new Set(
    weapons.filter(({ turret }) => turret === modules.Turret).map(({ gun }) => gun),
  );
  const turretIds = new Set(weapons.map(({ turret }) => turret));
  const suspension = options?.modules.Chassis?.find(({ id }) => id === modules.Chassis);
  const weight = moduleFields.reduce(
    (mass, [kind]) =>
      Math.fround(
        mass +
          Math.fround(options?.modules[kind]?.find(({ id }) => id === modules[kind])?.weight ?? 0),
      ),
    Math.fround(options?.hull_weight ?? 0),
  );
  const weightLimit = suspension?.max_weight;
  const validWeight =
    (ownedVehicle && !Object.keys(loadout?.modules ?? {}).length) ||
    weightLimit == null ||
    weight <= Math.fround(weightLimit);
  const capacity = weapon?.ammunition_capacity ?? options?.ammunition_capacity ?? 0;
  const shells = weapon?.ammunition ?? options?.ammunition ?? [];
  const currentAmmo = loadout?.ammunition ?? current?.ammunition;
  const compatibleAmmo =
    currentAmmo?.every(([id]) => shells.some((shell) => shell.id === id)) &&
    currentAmmo.reduce((sum, [, count]) => sum + count, 0) <= capacity;
  const startingAmmo = new Map(compatibleAmmo ? currentAmmo : undefined);
  const ammunition = shells.map(({ id, name, default_count }) => ({
    id,
    name,
    value: ammoEdits[id] ?? String(compatibleAmmo ? (startingAmmo.get(id) ?? 0) : default_count),
  }));
  const crew = (options?.crew ?? []).map((seat, slot) => ({
    ...seat,
    selected: loadout?.crew_perks?.[slot] ?? current?.crew_perks?.[slot] ?? [],
    efficiency:
      crewEdits[slot] ??
      loadout?.crew_perk_efficiency?.[slot] ??
      current?.crew_perk_efficiency?.[slot] ??
      100,
  }));
  const equipment = loadout?.equipment ?? current?.equipment ?? [];
  const consumables = loadout?.consumables ?? current?.consumables ?? [];
  const baseHitPoints = weapon?.hit_points ?? options?.stock_hit_points;
  // Match the server’s ordered f32 multiplications, then its ten-HP rounding.
  const equipmentHealth = equipment.reduce(
    (factor, id, slot) =>
      Math.fround(factor * Math.fround(options?.equipment_health_factors[id]?.[slot] ?? 1)),
    1,
  );
  const healthFactor = consumables.reduce(
    (factor, id) => Math.fround(factor * Math.fround(options?.consumable_health_factors[id] ?? 1)),
    equipmentHealth,
  );
  const adjustedHitPoints =
    baseHitPoints === undefined || healthFactor === 1
      ? baseHitPoints
      : Math.min(
          maxHitPoints,
          Math.ceil(Math.fround(Math.round(Math.fround(baseHitPoints * healthFactor)) / 10)) * 10,
        );
  const equippedHitPoints =
    modules.Turret === baseModules.Turret && !loadout?.equipment && !loadout?.consumables
      ? (options?.equipped_hit_points ?? adjustedHitPoints)
      : adjustedHitPoints;
  const rounds = ammunition.reduce((sum, { value }) => sum + Number(value), 0);
  const validHealth = !hitPoints || validInteger(hitPoints, 1, maxHitPoints);
  const validAmmo = ammunition.every(({ value }) => validInteger(value, 0, capacity));

  function changeModule(kind: BattleModuleKind, value: number | "") {
    if (vehicle === undefined || value === "") return;
    const next = { ...loadout?.modules, [kind]: value };
    if (kind === "Turret") {
      const choices = weapons.filter(({ turret }) => turret === value);
      if (!choices.some(({ gun }) => gun === modules.Gun)) {
        const compatible = choices.find(({ gun }) => gun === baseModules.Gun) ?? choices[0];
        if (compatible) next.Gun = compatible.gun;
      }
    }
    for (const [key] of moduleFields) {
      if (next[key] === baseModules[key]) delete next[key];
    }
    const weaponChanged = kind === "Gun" || kind === "Turret";
    if (weaponChanged) setAmmoEdits({});
    setLoadout({
      ...loadout,
      vehicle,
      modules: next,
      ammunition: weaponChanged ? undefined : loadout?.ammunition,
    });
  }

  function applyLoadout() {
    if (vehicle === undefined) return;
    const editedAmmo = ammoEdits && Object.keys(ammoEdits).length > 0;
    onSave({
      ...loadout,
      vehicle,
      hit_points: hitPoints ? Number(hitPoints) : undefined,
      ammunition: editedAmmo
        ? ammunition.map(({ id, value }) => [id, Number(value)])
        : loadout?.ammunition,
      crew_perk_efficiency: Object.keys(crewEdits).length
        ? crew.map(({ efficiency }, slot) =>
            crewEdits[slot] === undefined
              ? (loadout?.crew_perk_efficiency?.[slot] ?? null)
              : efficiency,
          )
        : loadout?.crew_perk_efficiency,
    });
  }

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="sm" aria-labelledby="battle-loadout-title">
      <DialogTitle id="battle-loadout-title">Customize {name}</DialogTitle>
      <DialogContent>
        {error && (
          <Alert
            severity="error"
            action={
              <Button
                onClick={() => {
                  setError("");
                  setAttempt((value) => value + 1);
                }}
              >
                Retry
              </Button>
            }
          >
            {error}
          </Alert>
        )}
        {!options && !error && (
          <CircularProgress size={24} aria-label="Loading battle vehicle loadout" />
        )}
        {options && (
          <Stack spacing={2} sx={{ pt: 1 }}>
            <Typography component="h3" variant="subtitle1">
              Modules
            </Typography>
            {moduleFields.map(([kind, label]) => {
              const choices = options.modules[kind]?.filter(({ id }) =>
                kind === "Gun" ? gunIds.has(id) : kind === "Turret" ? turretIds.has(id) : true,
              );
              return choices?.length ? (
                <ChoiceSelect
                  key={kind}
                  label={label}
                  value={modules[kind] ?? ""}
                  choices={choices}
                  onChange={(id) => changeModule(kind, id)}
                />
              ) : null;
            })}
            {weightLimit != null &&
              (validWeight ? (
                <Typography variant="body2" color="text.secondary">
                  Module weight {weight.toLocaleString()} / {weightLimit.toLocaleString()} kg
                </Typography>
              ) : (
                <Alert severity="error">
                  Module weight {weight.toLocaleString()} / {weightLimit.toLocaleString()} kg.
                  Select a suspension that supports the load or choose lighter modules.
                </Alert>
              ))}
            <TextField
              label="Hit points"
              type="number"
              value={hitPoints ?? (ownedVehicle ? (equippedHitPoints?.toString() ?? "") : "")}
              placeholder={equippedHitPoints?.toString()}
              error={!validHealth}
              helperText={
                validHealth
                  ? `Equipped HP ${equippedHitPoints}. Leave blank to use this value.`
                  : "Enter a whole number from 1 to 32,767."
              }
              slotProps={{ htmlInput: { min: 1, max: maxHitPoints, step: 1 } }}
              onChange={(event) => setHitPoints(event.target.value)}
            />
            <Typography component="h3" variant="subtitle1">
              Ammunition
            </Typography>
            {ammunition.map(({ id, name, value }) => {
              const valid = validInteger(value, 0, capacity);
              return (
                <TextField
                  key={id}
                  label={`${name} rounds`}
                  type="number"
                  value={value}
                  error={!valid}
                  helperText={valid ? undefined : `Enter a whole number from 0 to ${capacity}.`}
                  slotProps={{ htmlInput: { min: 0, max: capacity, step: 1 } }}
                  onChange={(event) => setAmmoEdits({ ...ammoEdits, [id]: event.target.value })}
                />
              );
            })}
            {validAmmo && (
              <Typography variant="body2" color={rounds > capacity ? "error" : "text.secondary"}>
                {rounds} / {capacity} rounds{rounds > capacity && " · over capacity"}
              </Typography>
            )}
            {options.equipment_slots > 0 && (
              <Typography component="h3" variant="subtitle1">
                Equipment
              </Typography>
            )}
            {Array.from({ length: options.equipment_slots }, (_, slot) => (
              <ChoiceSelect
                key={slot}
                label={`Equipment slot ${slot + 1}`}
                placeholder="Empty"
                value={equipment[slot] || ""}
                choices={options.equipment.filter(
                  ({ id }) => equipment[slot] === id || !equipment.includes(id),
                )}
                onChange={(id) =>
                  vehicle !== undefined &&
                  setLoadout({
                    ...loadout,
                    vehicle,
                    equipment: replaceSlot(equipment, slot, id || 0),
                  })
                }
              />
            ))}
            {options.consumable_slots > 0 && (
              <Typography component="h3" variant="subtitle1">
                Consumables
              </Typography>
            )}
            {Array.from({ length: options.consumable_slots }, (_, slot) => (
              <ChoiceSelect
                key={slot}
                label={`Consumable slot ${slot + 1}`}
                placeholder="Empty"
                value={consumables[slot] || ""}
                choices={options.consumables.filter(
                  ({ id }) => consumables[slot] === id || !consumables.includes(id),
                )}
                onChange={(id) =>
                  vehicle !== undefined &&
                  setLoadout({
                    ...loadout,
                    vehicle,
                    consumables: replaceSlot(consumables, slot, id || 0),
                  })
                }
              />
            ))}
            <Typography component="h3" variant="subtitle1">
              Crew
            </Typography>
            <Typography variant="body2" color="text.secondary">
              Perk effects scale with each crew member’s perk efficiency.
            </Typography>
            {crew.map(({ name: role, perks, efficiency, selected }, slot) => (
              <Fragment key={slot}>
                <Stack>
                  <Stack direction="row" justifyContent="space-between" spacing={2}>
                    <Typography id={`${crewLabelId}-${slot}`} variant="body2">
                      {role} perk efficiency (seat {slot + 1})
                    </Typography>
                    <Typography variant="body2" aria-hidden="true">
                      {efficiency}%
                    </Typography>
                  </Stack>
                  <Slider
                    aria-labelledby={`${crewLabelId}-${slot}`}
                    getAriaValueText={(value) => `${value}%`}
                    value={efficiency}
                    min={0}
                    max={100}
                    step={1}
                    onChange={(_, value) => {
                      if (typeof value === "number") {
                        setCrewEdits({ ...crewEdits, [slot]: value });
                      }
                    }}
                  />
                </Stack>
                <Autocomplete
                  multiple
                  options={perks}
                  getOptionLabel={({ name }) => name}
                  getOptionKey={({ id }) => id}
                  value={selected.flatMap((id) => perks.filter((perk) => perk.id === id))}
                  getOptionDisabled={({ id }) => selected.length >= 6 && !selected.includes(id)}
                  onChange={(_, picks) => {
                    if (vehicle === undefined) return;
                    const crewPerks = options.crew.map((_, index) =>
                      index === slot
                        ? picks.map(({ id }) => id)
                        : (loadout?.crew_perks?.[index] ?? null),
                    );
                    setLoadout({ ...loadout, vehicle, crew_perks: crewPerks });
                  }}
                  renderInput={(params) => (
                    <TextField {...params} label={`${role} perks (seat ${slot + 1})`} />
                  )}
                />
              </Fragment>
            ))}
          </Stack>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <Button
          variant="contained"
          disabled={
            !options || !weapon || !validWeight || !validHealth || !validAmmo || rounds > capacity
          }
          onClick={applyLoadout}
        >
          Apply loadout
        </Button>
      </DialogActions>
    </Dialog>
  );
}
