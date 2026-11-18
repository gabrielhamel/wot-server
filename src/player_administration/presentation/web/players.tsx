import type { PlayerRecord } from "../../application/admin-contracts";
import { PlayerRoleAction } from "./player-role";
import { Box, Typography } from "@mui/material";
import {
  Create,
  DataTable,
  List,
  PasswordInput,
  SaveButton,
  SimpleForm,
  TextInput,
  Toolbar,
  required,
} from "react-admin";

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

const createToolbar = (
  <Toolbar sx={{ borderTop: 1, borderColor: "divider" }}>
    <SaveButton label="Create player" />
  </Toolbar>
);

export function PlayerCreate() {
  return (
    <Create title="Create player" redirect="list" sx={{ maxWidth: 680 }}>
      <SimpleForm sanitizeEmptyValues toolbar={createToolbar} sx={{ p: { xs: 2, sm: 3 } }}>
        <Box sx={{ mb: 2 }}>
          <Typography color="text.secondary" variant="body2">
            The player receives tier I tech tree tanks with crew and ammunition.
          </Typography>
        </Box>
        <TextInput
          source="username"
          label="Login"
          helperText="Case-sensitive. Use the email address entered in the game client."
          autoComplete="off"
          validate={required()}
          fullWidth
        />
        <TextInput
          source="display_name"
          label="Display name"
          helperText="The name other players see."
          autoComplete="off"
          validate={required()}
          fullWidth
        />
        <PasswordInput
          source="password"
          label="Password"
          helperText={false}
          autoComplete="new-password"
          validate={required()}
          fullWidth
        />
      </SimpleForm>
    </Create>
  );
}
