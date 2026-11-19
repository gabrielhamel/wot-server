import { Alert, Button, Stack, Typography } from "@mui/material";
import { registerPlayer } from "../../infrastructure/admin-api";
import { PlayerAccountForm } from "./player-account-form";

export function RegistrationForm({
  onComplete,
  onBack,
}: {
  onComplete: (email: string) => void;
  onBack: () => void;
}) {
  return (
    <PlayerAccountForm
      mode="registration"
      onBack={onBack}
      onCreate={async (credentials) => {
        await registerPlayer({ ...credentials, confirm_password: credentials.password });
        onComplete(credentials.email);
      }}
    />
  );
}

export function RegistrationConfirmation({ email, onBack }: { email: string; onBack: () => void }) {
  return (
    <Stack spacing={2}>
      <Alert severity="success">Your account is ready to use.</Alert>
      <Typography component="h2" variant="h6">
        How to sign in to World of Tanks
      </Typography>
      <Typography component="ol" sx={{ m: 0, pl: 3 }}>
        <li>Open the World of Tanks client configured for this server.</li>
        <li>
          If Game Center signs you in automatically, choose <strong>Switch Account</strong>.
        </li>
        <li>
          Select <strong>WoT Server</strong> and sign in with <strong>{email}</strong> and the
          password you created.
        </li>
      </Typography>
      <Button variant="contained" onClick={onBack}>
        Sign in to the panel
      </Button>
    </Stack>
  );
}
