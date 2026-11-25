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
  CustomBattleLoadout,
  CustomBattleLoadoutOptions,
} from "../../application/admin-contracts";
import { getBattleLoadout } from "../../infrastructure/admin-api";
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

export function BattleLoadoutConfiguration({
  name,
  initial,
  onClose,
  onSave,
}: {
  name: string;
  initial: CustomBattleLoadout;
  onClose: () => void;
  onSave: (value: CustomBattleLoadout) => void;
}) {
  const [loadout, setLoadout] = useState(initial);
  const [hitPoints, setHitPoints] = useState(initial.hit_points?.toString() ?? "");
  const [ammoEdits, setAmmoEdits] = useState<Record<number, string>>({});
  const [crewEdits, setCrewEdits] = useState<Record<number, number>>({});
  const crewLabelId = useId();
  const [options, setOptions] = useState<CustomBattleLoadoutOptions>();
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    getBattleLoadout(initial.vehicle, controller.signal).then(
      (result) => {
        if (!controller.signal.aborted) setOptions(result);
      },
      (failure) => {
        if (!controller.signal.aborted)
          setError(errorMessage(failure, "Could not load battle vehicle customization."));
      },
    );
    return () => controller.abort();
  }, [attempt, initial.vehicle]);

  const initialAmmo = new Map(initial.ammunition);
  const ammunition = (options?.ammunition ?? []).map(({ id, name, default_count }) => ({
    id,
    name,
    value: ammoEdits[id] ?? String(initial.ammunition ? (initialAmmo.get(id) ?? 0) : default_count),
  }));
  const crew = (options?.crew ?? []).map((seat, slot) => ({
    ...seat,
    efficiency: crewEdits[slot] ?? initial.crew_perk_efficiency?.[slot] ?? 100,
  }));
  const capacity = options?.ammunition_capacity ?? 0;
  const rounds = ammunition.reduce((sum, { value }) => sum + Number(value), 0);
  const validHealth = hitPoints === "" || validInteger(hitPoints, 1, maxHitPoints);
  const validAmmo = ammunition.every(({ value }) => validInteger(value, 0, capacity));

  function applyLoadout() {
    onSave({
      ...loadout,
      hit_points: hitPoints === "" ? undefined : Number(hitPoints),
      ammunition: Object.keys(ammoEdits).length
        ? ammunition.map(({ id, value }) => [id, Number(value)])
        : initial.ammunition,
      crew_perk_efficiency: Object.keys(crewEdits).length
        ? crew.map(({ efficiency }) => efficiency)
        : initial.crew_perk_efficiency,
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
            <TextField
              label="Hit points"
              type="number"
              value={hitPoints}
              placeholder={String(options.stock_hit_points)}
              error={!validHealth}
              helperText={
                validHealth
                  ? `Stock HP: ${options.stock_hit_points}. Leave blank to use equipped HP.`
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
                value={loadout.equipment[slot] || ""}
                choices={options.equipment.filter(
                  ({ id }) => loadout.equipment[slot] === id || !loadout.equipment.includes(id),
                )}
                onChange={(id) =>
                  setLoadout({
                    ...loadout,
                    equipment: replaceSlot(loadout.equipment, slot, id || 0),
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
                value={loadout.consumables[slot] || ""}
                choices={options.consumables.filter(
                  ({ id }) => loadout.consumables[slot] === id || !loadout.consumables.includes(id),
                )}
                onChange={(id) =>
                  setLoadout({
                    ...loadout,
                    consumables: replaceSlot(loadout.consumables, slot, id || 0),
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
            {crew.map(({ name: role, perks, efficiency }, slot) => (
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
                  value={perks.filter(({ id }) => loadout.crew_perks[slot]?.includes(id))}
                  getOptionDisabled={({ id }) => {
                    const selected = loadout.crew_perks[slot] ?? [];
                    return selected.length >= 6 && !selected.includes(id);
                  }}
                  onChange={(_, selected) => {
                    const crewPerks = options.crew.map((_, index) =>
                      index === slot
                        ? selected.map(({ id }) => id)
                        : (loadout.crew_perks[index] ?? []),
                    );
                    setLoadout({ ...loadout, crew_perks: crewPerks });
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
          disabled={!options || !validHealth || !validAmmo || rounds > capacity}
          onClick={applyLoadout}
        >
          Apply loadout
        </Button>
      </DialogActions>
    </Dialog>
  );
}
