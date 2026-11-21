import { useEffect, useState } from "react";
import type { FormEvent } from "react";
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
import { errorMessage } from "../../../../platform/infrastructure/web/http";
import type {
  DepotGrantKind,
  DepotItem,
  Garage,
  Grant,
} from "../../../application/admin-contracts";
import type { AdminDataProvider } from "../../../infrastructure/admin-api";

const maxGrantCount = 100;
const labels = {
  crew_book: {
    heading: "Grant crew books",
    picker: "Crew book",
    action: "Grant crew book",
    unnamed: "Unnamed crew book",
  },
  bounty_equipment: {
    heading: "Grant Bounty Equipment",
    picker: "Bounty Equipment",
    action: "Grant Bounty Equipment",
    unnamed: "Unnamed Bounty Equipment",
  },
  improved_equipment: {
    heading: "Grant Improved Equipment",
    picker: "Improved Equipment",
    action: "Grant Improved Equipment",
    unnamed: "Unnamed Improved Equipment",
  },
  experimental_equipment: {
    heading: "Grant Experimental Equipment",
    picker: "Experimental Equipment",
    action: "Grant equipment",
    unnamed: "Unnamed Experimental Equipment",
  },
};

export function DepotGrant({
  api,
  busy,
  kind,
  depot,
  onGrant,
}: {
  api: AdminDataProvider;
  busy: boolean;
  kind: DepotGrantKind;
  depot: Garage["depot"];
  onGrant: (grant: Grant) => Promise<boolean>;
}) {
  const label = labels[kind];
  const [catalog, setCatalog] = useState<DepotItem[] | null>(null);
  const [loadError, setLoadError] = useState("");
  const [item, setItem] = useState<DepotItem | null>(null);
  const [count, setCount] = useState("1");

  useEffect(() => {
    const controller = new AbortController();
    api
      .getDepotCatalog(kind, controller.signal)
      .then(setCatalog)
      .catch((error) => {
        if (!controller.signal.aborted) setLoadError(errorMessage(error));
      });
    return () => controller.abort();
  }, [api, kind]);

  const value = Number(count);
  const valid = /^\d+$/.test(count) && value >= 1 && value <= maxGrantCount;
  const ownedCount = (id: number) => (depot[id] ?? 0).toLocaleString();

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy || !item || !valid) return;
    if (await onGrant({ kind, id: item.id, count: value })) setCount("1");
  }

  return (
    <Card component="form" aria-label={label.heading} onSubmit={submit}>
      <CardContent component={Stack} spacing={2}>
        <Typography variant="h6" component="h2">
          {label.heading}
        </Typography>
        {loadError && <Alert severity="error">{loadError}</Alert>}
        <Autocomplete
          options={catalog ?? []}
          value={item}
          onChange={(_, selection) => {
            setItem(selection);
            setCount("1");
          }}
          getOptionKey={(option) => option.id}
          isOptionEqualToValue={(a, b) => a.id === b.id}
          getOptionLabel={(option) => option.name || label.unnamed}
          loading={catalog === null && !loadError}
          disabled={busy || !!loadError}
          noOptionsText={
            catalog?.length ? "No matching items" : "No grantable items in the server catalog"
          }
          renderOption={(props, option) => {
            const { key, ...attributes } = props;
            return (
              <li key={key} {...attributes}>
                <Stack>
                  <span>{option.name || label.unnamed}</span>
                  <Typography variant="caption" color="text.secondary">
                    {ownedCount(option.id)} in depot
                  </Typography>
                </Stack>
              </li>
            );
          }}
          renderInput={(params) => (
            <TextField {...params} label={label.picker} placeholder="Search by name" />
          )}
        />
        <TextField
          label="Copies to add"
          value={count}
          onChange={(event) => setCount(event.target.value)}
          slotProps={{ htmlInput: { inputMode: "numeric", maxLength: 3 } }}
          helperText={item && `${ownedCount(item.id)} in depot · Up to ${maxGrantCount} per grant`}
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
          {label.action}
        </Button>
      </CardContent>
    </Card>
  );
}
