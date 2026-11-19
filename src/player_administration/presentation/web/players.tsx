import { Alert, Box, Button, Stack, Typography } from "@mui/material";
import { useState } from "react";
import { Create, DataTable, List, useRedirect, useRefresh } from "react-admin";
import type { PlayerCredentials, PlayerRecord } from "../../application/admin-contracts";
import { createManagedPlayer } from "../../infrastructure/admin-api";
import { PlayerAccountForm } from "./player-account-form";
import { PlayerRoleAction } from "./player-role";

export function PlayerList() {
  return (
    <List title="Player accounts" perPage={25} exporter={false} emptyWhileLoading>
      <DataTable bulkActionButtons={false} rowClick="show" sx={{ overflowX: "auto" }}>
        <DataTable.Col source="display_name" label="Display name" disableSort />
        <DataTable.Col source="username" label="Login" disableSort />
        <DataTable.Col<PlayerRecord>
          source="role"
          label="Role"
          disableSort
          render={(player) => (player.role === "admin" ? "Administrator" : "Player")}
        />
        <DataTable.Col label="Change role">
          <PlayerRoleAction />
        </DataTable.Col>
      </DataTable>
    </List>
  );
}

export function PlayerCreate() {
  const redirect = useRedirect();
  const refresh = useRefresh();
  const [credentials, setCredentials] = useState<PlayerCredentials>();
  const [copyStatus, setCopyStatus] = useState("");

  function backToPlayers() {
    setCredentials(undefined);
    redirect("list", "players");
  }

  async function copyPassword(password: string) {
    try {
      await navigator.clipboard.writeText(password);
      setCopyStatus("Password copied.");
    } catch {
      setCopyStatus("Select the password to copy it manually.");
    }
  }

  return (
    <Create title="Create player" sx={{ maxWidth: 680 }}>
      <Stack spacing={2} sx={{ p: { xs: 2, sm: 3 } }}>
        {credentials ? (
          <>
            <Alert severity="success">Player account created.</Alert>
            <Typography variant="body2">Sign-in email: {credentials.email}</Typography>
            <Box role="group" aria-label="Generated password">
              <Typography component="span">Generated password</Typography>
              <Box component="code" sx={{ display: "block", overflowWrap: "anywhere", mt: 1 }}>
                {credentials.password}
              </Box>
            </Box>
            <Stack direction="row" spacing={1}>
              <Button variant="outlined" onClick={() => void copyPassword(credentials.password)}>
                Copy password
              </Button>
              <Button variant="contained" onClick={backToPlayers}>
                Done
              </Button>
            </Stack>
            <Typography role="status" variant="body2">
              {copyStatus}
            </Typography>
          </>
        ) : (
          <PlayerAccountForm
            mode="managed"
            onBack={backToPlayers}
            onCreate={async (created) => {
              await createManagedPlayer(created);
              setCredentials(created);
              refresh();
            }}
          />
        )}
      </Stack>
    </Create>
  );
}
