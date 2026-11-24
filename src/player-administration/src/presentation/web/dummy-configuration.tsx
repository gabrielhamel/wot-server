import {
  Alert,
  Autocomplete,
  Button,
  CircularProgress,
  Dialog,
  DialogActions,
  DialogContent,
  DialogTitle,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { useEffect, useState } from "react";
import type { DummyLoadout, DummyLoadoutOptions } from "../../application/admin-contracts";
import { getDummyLoadout } from "../../infrastructure/admin-api";
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

export function DummyConfiguration({
  name,
  initial,
  onClose,
  onSave,
}: {
  name: string;
  initial: DummyLoadout;
  onClose: () => void;
  onSave: (value: DummyLoadout) => void;
}) {
  const [loadout, setLoadout] = useState(initial);
  const [options, setOptions] = useState<DummyLoadoutOptions>();
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    getDummyLoadout(initial.vehicle, controller.signal).then(
      (result) => {
        if (!controller.signal.aborted) setOptions(result);
      },
      (failure) => {
        if (!controller.signal.aborted)
          setError(errorMessage(failure, "Could not load dummy customization."));
      },
    );
    return () => controller.abort();
  }, [attempt, initial.vehicle]);

  return (
    <Dialog open onClose={onClose} fullWidth maxWidth="sm" aria-labelledby="dummy-loadout-title">
      <DialogTitle id="dummy-loadout-title">Customize {name}</DialogTitle>
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
        {!options && !error && <CircularProgress size={24} aria-label="Loading dummy loadout" />}
        {options && (
          <Stack spacing={2} sx={{ pt: 1 }}>
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
              Crew perks · 100% crew
            </Typography>
            {options.crew.map(({ name: role, perks }, slot) => (
              <Autocomplete
                key={slot}
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
            ))}
          </Stack>
        )}
      </DialogContent>
      <DialogActions>
        <Button onClick={onClose}>Cancel</Button>
        <Button variant="contained" disabled={!options} onClick={() => onSave(loadout)}>
          Apply loadout
        </Button>
      </DialogActions>
    </Dialog>
  );
}
