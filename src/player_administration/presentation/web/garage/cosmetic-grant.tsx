import { useEffect, useState } from "react";
import {
  Alert,
  Autocomplete,
  Button,
  Card,
  CardContent,
  Stack,
  TextField,
  Typography,
} from "@mui/material";

import type { FormEvent } from "react";
import { errorMessage } from "../../../../platform/infrastructure/web/http";
import type {
  Cosmetic,
  GarageVehicle,
  Grant,
  OwnedCosmetics,
} from "../../../application/admin-contracts";
import type { AdminDataProvider } from "../../../infrastructure/admin-api";
import { vehicleName } from "./garage-inventory";

interface CosmeticGrantProps {
  api: AdminDataProvider;
  busy: boolean;
  onGrant: (grant: Grant) => Promise<boolean>;
  kind: "attachment" | "stat_tracker";
  owned?: OwnedCosmetics;
  vehicles?: GarageVehicle[];
}

const maxGrantCount = 100;

export function CosmeticGrant({ api, busy, kind, owned, vehicles, onGrant }: CosmeticGrantProps) {
  const bound = kind === "stat_tracker";
  const title = bound ? "Stat tracker" : "3D attachment";
  const itemLabel = bound ? "stat tracker" : "attachment";
  const [catalog, setCatalog] = useState<Cosmetic[] | null>(null);
  const [loadError, setLoadError] = useState("");
  const [selected, setSelected] = useState<number | null>(null);
  const [vehicleId, setVehicleId] = useState<number | null>(null);
  const [count, setCount] = useState("1");

  useEffect(() => {
    const controller = new AbortController();
    const load = bound ? api.getStatTrackerCatalog : api.getAttachmentCatalog;
    load(controller.signal)
      .then(setCatalog)
      .catch((error) => {
        if (!controller.signal.aborted) setLoadError(errorMessage(error));
      });
    return () => controller.abort();
  }, [api, bound]);

  const compatibleVehicles =
    vehicles?.filter((tank) => Object.keys(tank.stat_trackers ?? {}).length > 0) ?? [];
  const vehicle = compatibleVehicles.find((tank) => tank.inventory_id === vehicleId) ?? null;
  const options =
    catalog?.filter((item) => !bound || Object.hasOwn(vehicle?.stat_trackers ?? {}, item.id)) ?? [];
  const item = options.find((option) => option.id === selected) ?? null;
  const ownedCount = (id: number) => (bound ? vehicle?.stat_trackers : owned)?.[id] ?? 0;
  const remaining = (item: Cosmetic) =>
    item.max_owned === null
      ? maxGrantCount
      : Math.max(0, Math.min(maxGrantCount, item.max_owned - ownedCount(item.id)));
  const limit = item ? remaining(item) : maxGrantCount;
  const value = Number(count);
  const valid = /^\d+$/.test(count) && value >= 1 && value <= limit;

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy || !item || !valid) return;
    if (bound && !vehicle) return;
    const grant: Grant =
      bound && vehicle
        ? { kind: "stat_tracker", vehicle: vehicle.inventory_id, id: item.id, count: value }
        : { kind: "attachment", id: item.id, count: value };
    if (await onGrant(grant)) {
      setSelected(null);
      setCount("1");
    }
  }

  return (
    <Card component="form" onSubmit={submit}>
      <CardContent component={Stack} spacing={2}>
        <Typography variant="h6" component="h2">
          Grant a {bound ? itemLabel : title}
        </Typography>
        {loadError && <Alert severity="error">{loadError}</Alert>}
        {bound && (
          <Autocomplete
            options={compatibleVehicles}
            value={vehicle}
            onChange={(_, tank) => {
              setVehicleId(tank?.inventory_id ?? null);
              setSelected(null);
              setCount("1");
            }}
            getOptionKey={(tank) => tank.inventory_id}
            isOptionEqualToValue={(a, b) => a.inventory_id === b.inventory_id}
            getOptionLabel={vehicleName}
            noOptionsText="This player has no compatible tanks"
            disabled={busy || !!loadError}
            renderInput={(params) => (
              <TextField {...params} label="Tank for stat tracker" placeholder="Search by name" />
            )}
          />
        )}
        <Autocomplete
          options={options}
          value={item}
          onChange={(_, selection) => {
            setSelected(selection?.id ?? null);
            setCount("1");
          }}
          getOptionKey={(item) => item.id}
          isOptionEqualToValue={(a, b) => a.id === b.id}
          getOptionLabel={(item) => item.name || `Unnamed ${itemLabel}`}
          getOptionDisabled={(item) => remaining(item) === 0}
          loading={catalog === null && !loadError}
          disabled={busy || !!loadError || (bound && !vehicle)}
          noOptionsText={
            catalog?.length === 0
              ? "No grantable items in the server catalog"
              : `No matching ${itemLabel}s`
          }
          renderOption={(props, item) => {
            const { key, ...attributes } = props;
            return (
              <li key={key} {...attributes}>
                <Stack>
                  <span>{item.name || `Unnamed ${itemLabel}`}</span>
                  <Typography variant="caption" color="text.secondary">
                    {ownedCount(item.id)} owned
                  </Typography>
                </Stack>
              </li>
            );
          }}
          renderInput={(params) => (
            <TextField {...params} label={title} placeholder="Search by name" />
          )}
        />
        <TextField
          label="Copies to add"
          value={count}
          onChange={(event) => setCount(event.target.value)}
          slotProps={{ htmlInput: { inputMode: "numeric", maxLength: 3 } }}
          helperText={item && `${ownedCount(item.id)} owned · Up to ${limit} per grant`}
          error={!!item && !valid}
          required
          disabled={busy || !item}
          fullWidth
        />
        <Button
          type="submit"
          variant="contained"
          disabled={busy || !item || !valid}
          sx={{ alignSelf: "start" }}
        >
          Grant {itemLabel}
        </Button>
      </CardContent>
    </Card>
  );
}
