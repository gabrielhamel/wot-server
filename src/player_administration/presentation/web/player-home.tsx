import { Alert, Button, CircularProgress, Container, Stack, Typography } from "@mui/material";
import { useEffect, useState } from "react";
import { useLogout } from "react-admin";
import type { PlayerGarage } from "../../application/admin-contracts";
import { getOwnGarage } from "../../infrastructure/admin-api";
import { errorMessage, responseStatus } from "../../../platform/infrastructure/web/http";
import { GarageBalances, OwnedTanks } from "./garage/garage-inventory";

export function PlayerHome({ displayName }: { displayName: string }) {
  const logout = useLogout();
  const [garage, setGarage] = useState<PlayerGarage>();
  const [error, setError] = useState("");
  const [logoutError, setLogoutError] = useState("");
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    getOwnGarage(controller.signal).then(
      (result) => {
        if (!controller.signal.aborted) setGarage(result);
      },
      (failure) => {
        if (controller.signal.aborted) return;
        if (responseStatus(failure) === 401) {
          void logout();
        } else {
          setError(errorMessage(failure, "Could not load your garage."));
        }
      },
    );
    return () => controller.abort();
  }, [attempt, logout]);

  async function signOut() {
    setLogoutError("");
    try {
      await logout({ revokeSession: true }, undefined, false);
    } catch (failure) {
      setLogoutError(errorMessage(failure, "Could not sign out."));
    }
  }

  return (
    <Container component="main" maxWidth="md" sx={{ py: 4 }}>
      <Stack spacing={3}>
        <Stack direction="row" gap={2} justifyContent="space-between" alignItems="start">
          <div>
            <Typography variant="body2" color="text.secondary">
              {displayName}
            </Typography>
            <Typography component="h1" variant="h4">
              My garage
            </Typography>
          </div>
          <Button onClick={signOut}>Sign out</Button>
        </Stack>
        {logoutError && <Alert severity="error">{logoutError}</Alert>}
        {error && (
          <Alert
            severity="error"
            action={
              <Button
                onClick={() => {
                  setError("");
                  setAttempt(attempt + 1);
                }}
              >
                Retry
              </Button>
            }
          >
            {error}
          </Alert>
        )}
        {!garage && !error && <CircularProgress size={24} aria-label="Loading your garage" />}
        {garage && (
          <>
            <GarageBalances garage={garage} />
            <OwnedTanks garage={garage} />
          </>
        )}
      </Stack>
    </Container>
  );
}
