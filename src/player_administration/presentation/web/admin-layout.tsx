import ShieldOutlined from "@mui/icons-material/ShieldOutlined";
import { Alert, Button, CircularProgress, MenuItem, Typography } from "@mui/material";
import {
  AppBar,
  Layout,
  TitlePortal,
  UserMenu,
  useGetIdentity,
  useLogout,
  useNotify,
  useUserMenu,
} from "react-admin";
import type { AppBarProps, LayoutProps } from "react-admin";
import { errorMessage } from "../../../platform/infrastructure/web/http";
import { PlayerHome } from "./player-home";

function SignOut() {
  const logout = useLogout();
  const notify = useNotify();
  const menu = useUserMenu();

  async function signOut() {
    menu?.onClose();
    try {
      await logout({ revokeSession: true }, undefined, false);
    } catch (error) {
      notify(errorMessage(error, "Could not sign out."), { type: "error" });
    }
  }

  return <MenuItem onClick={signOut}>Sign out</MenuItem>;
}

function ServerBar(props: AppBarProps) {
  return (
    <AppBar
      {...props}
      userMenu={
        <UserMenu>
          <SignOut />
        </UserMenu>
      }
      color="transparent"
      elevation={0}
      sx={{ bgcolor: "background.paper", borderBottom: 1, borderColor: "divider" }}
    >
      <ShieldOutlined sx={{ mr: 1, color: "primary.main", display: { xs: "none", sm: "block" } }} />
      <Typography variant="subtitle2" sx={{ mr: 3, display: { xs: "none", md: "block" } }}>
        World of Tanks Server
      </Typography>
      <TitlePortal />
    </AppBar>
  );
}

export function ServerLayout(props: LayoutProps) {
  const { identity, isPending, error, refetch } = useGetIdentity();
  if (isPending)
    return <CircularProgress size={24} aria-label="Loading your account" sx={{ m: 3 }} />;
  if (error || (identity?.role !== "admin" && identity?.role !== "player")) {
    return (
      <Alert severity="error" action={<Button onClick={() => void refetch()}>Retry</Button>}>
        Could not load your account.
      </Alert>
    );
  }
  if (identity.role === "player")
    return <PlayerHome key={identity.id} displayName={identity.fullName ?? "Player"} />;
  return (
    <Layout
      {...props}
      appBar={ServerBar}
      appBarAlwaysOn
      sx={{ "& .RaLayout-content": { px: { xs: 1, sm: 3 }, pt: { xs: 1, sm: 2 } } }}
    />
  );
}
