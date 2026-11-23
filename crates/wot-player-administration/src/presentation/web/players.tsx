import { Alert, Box, Button, Stack, TextField, Typography } from "@mui/material";
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

  async function copyCredential(field: "Email" | "Password", value: string) {
    try {
      await navigator.clipboard.writeText(value);
      setCopyStatus(`${field} copied.`);
    } catch {
      setCopyStatus(`Select the ${field.toLowerCase()} to copy it manually.`);
    }
  }

  return (
    <Create title="Create player" sx={{ maxWidth: 680 }}>
      <Stack spacing={2} sx={{ p: { xs: 2, sm: 3 } }}>
        {credentials ? (
          <>
            <Alert severity="success">Player account created.</Alert>
            <Box sx={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto", gap: 1 }}>
              <TextField
                label="Sign-in email"
                value={credentials.email}
                slotProps={{ input: { readOnly: true } }}
              />
              <Button
                variant="outlined"
                onClick={() => void copyCredential("Email", credentials.email)}
              >
                Copy email
              </Button>
              <TextField
                label="Generated password"
                value={credentials.password}
                slotProps={{ input: { readOnly: true } }}
              />
              <Button
                variant="outlined"
                onClick={() => void copyCredential("Password", credentials.password)}
              >
                Copy password
              </Button>
            </Box>
            <Button variant="contained" onClick={backToPlayers}>
              Done
            </Button>
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
