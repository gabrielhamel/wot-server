import { useEffect, useState } from "react";
import { Alert, Box, Button, CircularProgress, Stack } from "@mui/material";
import { ListButton, Title, useDataProvider, useGetRecordId } from "react-admin";
import type { Identifier } from "react-admin";
import type { Garage, Grant } from "../../../application/admin-contracts";
import type { AdminDataProvider } from "../../../infrastructure/admin-api";
import { errorMessage } from "../../../../../platform/src/infrastructure/web/http";
import { CosmeticGrant } from "./cosmetic-grant";
import { DepotGrant } from "./depot-grant";
import { CurrencyGrant } from "./currency-grant";
import { TankGrant } from "./tank-grant";
import { GarageBalances, OwnedTanks } from "./garage-inventory";

export function PlayerGarage() {
  const id = useGetRecordId();
  return <GaragePage key={id} id={id} />;
}

function GaragePage({ id }: { id: Identifier }) {
  const api = useDataProvider<AdminDataProvider>();
  const [garage, setGarage] = useState<Garage | null>(null);
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const [feedback, setFeedback] = useState<{
    severity: "success" | "error";
    message: string;
  } | null>(null);
  const [refresh, setRefresh] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    api
      .getGarage(id, controller.signal)
      .then(setGarage)
      .catch((error) => {
        if (!controller.signal.aborted) setLoadError(errorMessage(error));
      });
    return () => controller.abort();
  }, [api, id, refresh]);

  async function submit(body: Grant) {
    setBusy(true);
    setFeedback(null);
    try {
      const result = await api.grant(id, body);
      setGarage(result.garage);
      setFeedback({
        severity: "success",
        message:
          result.delivery === "queued_for_garage"
            ? "Grant applied. A live garage update was queued."
            : "Grant applied. The player will see it when the garage next syncs.",
      });
      return true;
    } catch (error) {
      setFeedback({ severity: "error", message: errorMessage(error) });
      return false;
    } finally {
      setBusy(false);
    }
  }

  function reload() {
    setGarage(null);
    setLoadError("");
    setRefresh((value) => value + 1);
  }

  return (
    <Stack spacing={2} sx={{ maxWidth: 1040, pb: 3 }}>
      <Title
        title={garage ? `${garage.display_name} · Resources & Garage` : "Resources & Garage"}
      />
      <Stack direction="row" justifyContent="space-between" alignItems="center">
        <ListButton resource="players" label="Player accounts" />
        <Button onClick={reload} disabled={busy || (!garage && !loadError)}>
          Refresh balances
        </Button>
      </Stack>
      {loadError && <Alert severity="error">{loadError}</Alert>}
      {!garage && !loadError && <CircularProgress aria-label="Loading player garage" size={24} />}
      {garage && (
        <>
          <GarageBalances garage={garage} />
          {feedback && (
            <Alert severity={feedback.severity} onClose={() => setFeedback(null)}>
              {feedback.message}
            </Alert>
          )}
          <Box sx={{ display: "grid", gridTemplateColumns: { xs: "1fr", md: "1fr 1fr" }, gap: 2 }}>
            <CurrencyGrant busy={busy} onGrant={submit} />
            <TankGrant busy={busy} garage={garage} onGrant={submit} />
            <DepotGrant
              api={api}
              busy={busy}
              kind="experimental_equipment"
              depot={garage.depot}
              onGrant={submit}
            />
            <DepotGrant
              api={api}
              busy={busy}
              kind="bounty_equipment"
              depot={garage.depot}
              onGrant={submit}
            />
            <DepotGrant
              api={api}
              busy={busy}
              kind="improved_equipment"
              depot={garage.depot}
              onGrant={submit}
            />
            <DepotGrant
              api={api}
              busy={busy}
              kind="crew_book"
              depot={garage.depot}
              onGrant={submit}
            />
            <CosmeticGrant
              api={api}
              busy={busy}
              kind="attachment"
              owned={garage.attachments}
              onGrant={submit}
            />
            <CosmeticGrant
              api={api}
              busy={busy}
              kind="stat_tracker"
              vehicles={garage.vehicles}
              onGrant={submit}
            />
          </Box>
          <OwnedTanks garage={garage} />
        </>
      )}
    </Stack>
  );
}
