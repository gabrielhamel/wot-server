import { useEffect, useState } from "react";
import {
  Alert,
  Autocomplete,
  Button,
  Card,
  CardContent,
  createFilterOptions,
  MenuItem,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import type { FormEvent } from "react";
import { errorMessage } from "../../../../../shared-infrastructure/src/infrastructure/web/http";
import type { Grant, OwnedCosmetics, SpecialRecruit } from "../../../application/admin-contracts";
import type { AdminDataProvider } from "../../../infrastructure/admin-api";

interface SpecialRecruitGrantProps {
  api: AdminDataProvider;
  busy: boolean;
  owned: OwnedCosmetics;
  onGrant: (grant: Grant) => Promise<boolean>;
}

const filterRecruits = createFilterOptions<SpecialRecruit>({ limit: 100 });

export function SpecialRecruitGrant({ api, busy, owned, onGrant }: SpecialRecruitGrantProps) {
  const [catalog, setCatalog] = useState<SpecialRecruit[] | null>(null);
  const [loadError, setLoadError] = useState("");
  const [recruit, setRecruit] = useState<SpecialRecruit | null>(null);
  const [countryId, setCountryId] = useState<number | "">("");

  useEffect(() => {
    const controller = new AbortController();
    api
      .getSpecialRecruitCatalog(controller.signal)
      .then(setCatalog)
      .catch((error) => {
        if (!controller.signal.aborted) setLoadError(errorMessage(error));
      });
    return () => controller.abort();
  }, [api]);

  const countries = recruit?.countries ?? [];
  const country = countries.find(({ id }) => id === countryId);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (busy || !country || owned[country.id]) return;
    if (await onGrant({ kind: "special_recruit", id: country.id })) {
      setRecruit(null);
      setCountryId("");
    }
  }

  return (
    <Card component="form" onSubmit={submit}>
      <CardContent component={Stack} spacing={2}>
        <Typography variant="h6" component="h2">
          Grant a special recruit
        </Typography>
        {loadError && <Alert severity="error">{loadError}</Alert>}
        <Autocomplete
          options={catalog ?? []}
          filterOptions={filterRecruits}
          value={recruit}
          onChange={(_, selection) => {
            setRecruit(selection);
            setCountryId(selection?.countries.length === 1 ? selection.countries[0].id : "");
          }}
          getOptionKey={(option) => option.id}
          getOptionLabel={(option) => option.name}
          getOptionDisabled={(option) => option.countries.every(({ id }) => !!owned[id])}
          loading={catalog === null && !loadError}
          disabled={busy || !!loadError}
          renderInput={(params) => (
            <TextField {...params} label="Special recruit" placeholder="Search by name" />
          )}
        />
        {recruit && (
          <TextField
            select
            label="Country"
            value={countryId}
            onChange={(event) => setCountryId(Number(event.target.value))}
            disabled={busy || countries.length === 1}
          >
            {countries.map(({ id, name }) => (
              <MenuItem key={id} value={id} disabled={!!owned[id]}>
                {name}
                {owned[id] ? " · Owned" : ""}
              </MenuItem>
            ))}
          </TextField>
        )}
        <Button
          type="submit"
          variant="contained"
          disabled={busy || !country || !!owned[country.id]}
          sx={{ alignSelf: "start" }}
        >
          Grant special recruit
        </Button>
      </CardContent>
    </Card>
  );
}
