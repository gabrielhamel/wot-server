import {
  Alert,
  Box,
  Button,
  CircularProgress,
  Container,
  Stack,
  Tab,
  Tabs,
  Typography,
} from "@mui/material";
import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import type { ReactNode } from "react";
import { TitlePortal, useLogout } from "react-admin";
import type { PlayerGarage } from "../../application/admin-contracts";
import { dashboardStoreTransport, getOwnGarage } from "../../infrastructure/admin-api";
import { errorMessage, responseStatus } from "../../../../wot-platform/src/infrastructure/web/http";
import { GarageBalances, OwnedTanks } from "./garage/garage-inventory";

const DashboardStore = lazy(async () => ({
  default: (await import("../../../../wot-player-sessions/src/web")).EmbeddedStore,
}));

const viewTitles = { garage: "My garage", store: "Store", admin: "Admin" };

type PlayerView = keyof typeof viewTitles;

export function PlayerHome({
  displayName,
  storeEnabled,
  administrator,
  view,
  onViewChange,
  children,
}: {
  displayName: string;
  storeEnabled: boolean;
  administrator: boolean;
  view: PlayerView;
  onViewChange: (view: PlayerView) => void;
  children: ReactNode;
}) {
  const logout = useLogout();
  const [garageVersion, setGarageVersion] = useState(0);
  const [logoutError, setLogoutError] = useState("");
  const invalidateGarage = useCallback(() => setGarageVersion((version) => version + 1), []);
  const signInAgain = useCallback(() => void logout(), [logout]);
  const showTabs = storeEnabled || administrator;

  async function signOut() {
    setLogoutError("");
    try {
      await logout({ revokeSession: true }, undefined, false);
    } catch (failure) {
      setLogoutError(errorMessage(failure, "Could not sign out."));
    }
  }

  return (
    <Container component="main" maxWidth={view === "garage" ? "md" : "lg"} sx={{ py: 4 }}>
      <Stack spacing={3}>
        <Stack direction="row" gap={2} justifyContent="space-between" alignItems="start">
          <div>
            <Typography variant="body2" color="text.secondary">
              {displayName}
            </Typography>
            <Typography component="h1" variant="h4">
              {viewTitles[view]}
            </Typography>
          </div>
          <Button onClick={signOut}>Sign out</Button>
        </Stack>
        {logoutError && <Alert severity="error">{logoutError}</Alert>}
        {showTabs && (
          <Tabs
            aria-label="Player dashboard"
            value={view}
            onChange={(_, next: PlayerView) => onViewChange(next)}
          >
            <Tab label="My garage" value="garage" id="player-garage" aria-controls="player-view" />
            {storeEnabled && (
              <Tab label="Store" value="store" id="player-store" aria-controls="player-view" />
            )}
            {administrator && (
              <Tab label="Admin" value="admin" id="player-admin" aria-controls="player-view" />
            )}
          </Tabs>
        )}
        <Box
          id="player-view"
          role={showTabs ? "tabpanel" : undefined}
          aria-labelledby={showTabs ? `player-${view}` : undefined}
        >
          {view === "store" && (
            <Suspense fallback={<CircularProgress size={24} aria-label="Loading Store" />}>
              <DashboardStore
                transport={dashboardStoreTransport}
                onUnauthorized={signInAgain}
                onPurchased={invalidateGarage}
              />
            </Suspense>
          )}
          {view === "garage" && <Garage key={garageVersion} onUnauthorized={signInAgain} />}
          {view === "admin" && (
            <>
              <TitlePortal component="h2" sx={{ mb: 2, minHeight: "1em" }} />
              {children}
            </>
          )}
        </Box>
      </Stack>
    </Container>
  );
}

function Garage({ onUnauthorized }: { onUnauthorized: () => void }) {
  const [garage, setGarage] = useState<PlayerGarage>();
  const [error, setError] = useState("");
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
          onUnauthorized();
        } else {
          setError(errorMessage(failure, "Could not load your garage."));
        }
      },
    );
    return () => controller.abort();
  }, [attempt, onUnauthorized]);

  return (
    <Stack spacing={3}>
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
  );
}
