import { Alert, Button, CircularProgress, Stack } from "@mui/material";
import { Link, useGetIdentity, useLocation, useNavigate } from "react-admin";
import type { LayoutProps } from "react-admin";
import { PlayerHome } from "./player-home";

const viewPaths = {
  garage: "/",
  store: "/?tab=store",
  admin: "/players",
};

export function ServerLayout({ children }: LayoutProps) {
  const { identity, isPending, error, refetch } = useGetIdentity();
  const { pathname, search } = useLocation();
  const navigate = useNavigate();
  if (isPending)
    return <CircularProgress size={24} aria-label="Loading your account" sx={{ m: 3 }} />;
  if (error || (identity?.role !== "admin" && identity?.role !== "player")) {
    return (
      <Alert severity="error" action={<Button onClick={() => void refetch()}>Retry</Button>}>
        Could not load your account.
      </Alert>
    );
  }

  const administrator = identity.role === "admin";
  const storeEnabled = identity.storeEnabled === true;
  const battleRoute = pathname === "/battles";
  const adminRoute =
    battleRoute || pathname === viewPaths.admin || pathname.startsWith(`${viewPaths.admin}/`);
  const view =
    administrator && adminRoute
      ? "admin"
      : storeEnabled && pathname === "/" && new URLSearchParams(search).get("tab") === "store"
        ? "store"
        : "garage";
  return (
    <PlayerHome
      key={identity.id}
      displayName={identity.fullName ?? "Player"}
      storeEnabled={storeEnabled}
      administrator={administrator}
      view={view}
      onViewChange={(next) => navigate(viewPaths[next])}
    >
      <Stack component="nav" aria-label="Administration" direction="row" spacing={1} sx={{ mb: 2 }}>
        <Button component={Link} to="/players" variant={battleRoute ? "text" : "outlined"}>
          Player accounts
        </Button>
        <Button
          component={Link}
          to={battleRoute ? { pathname, search } : "/battles"}
          variant={battleRoute ? "outlined" : "text"}
        >
          Battles
        </Button>
      </Stack>
      {children}
    </PlayerHome>
  );
}
