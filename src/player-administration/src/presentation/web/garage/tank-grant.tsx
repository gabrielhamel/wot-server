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
import { useDataProvider } from "react-admin";
import type { CatalogVehicle, Garage, Grant } from "../../../application/admin-contracts";
import type { AdminDataProvider } from "../../../infrastructure/admin-api";
import { errorMessage } from "../../../../../platform/src/infrastructure/web/http";
import { vehicleName } from "./garage-inventory";

export function TankGrant({
  busy,
  garage,
  onGrant,
}: {
  busy: boolean;
  garage: Garage;
  onGrant: (grant: Grant) => Promise<boolean>;
}) {
  const api = useDataProvider<AdminDataProvider>();
  const [catalog, setCatalog] = useState<CatalogVehicle[] | null>(null);
  const [catalogError, setCatalogError] = useState("");
  const [vehicle, setVehicle] = useState<CatalogVehicle | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    api
      .getVehicleCatalog(controller.signal)
      .then(setCatalog)
      .catch((error) => {
        if (!controller.signal.aborted) setCatalogError(errorMessage(error));
      });
    return () => controller.abort();
  }, [api]);

  const owned = new Set(garage.vehicles.map((tank) => tank.key));
  const full = garage.vehicles.length >= garage.slots;

  return (
    <Card
      component="form"
      onSubmit={async (event) => {
        event.preventDefault();
        if (
          vehicle &&
          !full &&
          !owned.has(vehicle.key) &&
          (await onGrant({ kind: "vehicle", vehicle: vehicle.id }))
        ) {
          setVehicle(null);
        }
      }}
    >
      <CardContent component={Stack} spacing={2}>
        <Typography variant="h6" component="h2">
          Grant a tank
        </Typography>
        {catalogError && <Alert severity="error">{catalogError}</Alert>}
        {full && <Alert severity="warning">The player has no free garage slots.</Alert>}
        <Autocomplete
          options={catalog ?? []}
          value={vehicle}
          onChange={(_, value) => setVehicle(value)}
          getOptionKey={(option) => option.key}
          isOptionEqualToValue={(a, b) => a.id === b.id}
          getOptionLabel={vehicleName}
          getOptionDisabled={(option) => owned.has(option.key)}
          loading={catalog === null && !catalogError}
          disabled={busy || !!full}
          renderOption={(props, option) => {
            const { key, ...attributes } = props;
            return (
              <li key={key} {...attributes}>
                <Stack>
                  <span>
                    {vehicleName(option)}
                    {owned.has(option.key) ? " · Owned" : ""}
                  </span>
                  <Typography variant="caption" color="text.secondary">
                    {option.tier === null ? "Tier unavailable" : `Tier ${option.tier}`}
                  </Typography>
                </Stack>
              </li>
            );
          }}
          renderInput={(params) => (
            <TextField {...params} label="Tank" placeholder="Search by name" />
          )}
        />
        <Button
          type="submit"
          variant="contained"
          disabled={busy || !vehicle || !!full || owned.has(vehicle.key)}
          sx={{ alignSelf: "start" }}
        >
          Grant tank
        </Button>
      </CardContent>
    </Card>
  );
}
