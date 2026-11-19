import { Alert, Button, Stack, TextField, Typography } from "@mui/material";
import { useEffect, useState } from "react";
import type { FormEvent } from "react";
import { errorMessage } from "../../../platform/infrastructure/web/http";
import { checkUsername, registerPlayer } from "../../infrastructure/admin-api";

const usernamePattern = "[A-Za-z0-9_]{3,24}";
const usernameHelp = "Use 3 to 24 letters, numbers or underscores.";
const passwordHelp = "Use 12 to 128 characters. A passphrase works well.";

type Availability = "checking" | "available" | "taken" | "unconfirmed";
const availabilityMessages: Record<Availability, string> = {
  checking: "Checking availability…",
  available: "Username available.",
  taken: "That username is already taken.",
  unconfirmed: "Availability will be checked when you submit.",
};

function useUsernameAvailability(username: string) {
  const valid = new RegExp(`^${usernamePattern}$`).test(username);
  const [result, setResult] = useState<{ username: string; status: Availability }>();

  useEffect(() => {
    if (!valid) return;
    const controller = new AbortController();
    const timer = window.setTimeout(() => {
      checkUsername(username, controller.signal).then(
        (available) => {
          if (!controller.signal.aborted)
            setResult({ username, status: available ? "available" : "taken" });
        },
        () => {
          if (!controller.signal.aborted) setResult({ username, status: "unconfirmed" });
        },
      );
    }, 250);
    return () => {
      window.clearTimeout(timer);
      controller.abort();
    };
  }, [username, valid]);

  if (valid) return result?.username === username ? result.status : "checking";
}

export function RegistrationForm({
  onComplete,
  onBack,
}: {
  onComplete: (email: string) => void;
  onBack: () => void;
}) {
  const [username, setUsername] = useState("");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const availability = useUsernameAvailability(username);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pending || availability === "taken") return;
    const data = new FormData(event.currentTarget);
    const email = String(data.get("email") ?? "");
    const password = String(data.get("password") ?? "");
    const confirm = String(data.get("confirm_password") ?? "");
    setError("");
    const length = Array.from(password).length;
    if (length < 12 || length > 128) {
      setError(passwordHelp);
      return;
    }
    if (password !== confirm) {
      setError("The passwords do not match.");
      return;
    }

    setPending(true);
    try {
      await registerPlayer({ email, username, password, confirm_password: confirm });
      onComplete(email);
    } catch (failure) {
      setError(errorMessage(failure, "Could not create your account."));
      setPending(false);
    }
  }

  return (
    <Stack component="form" onSubmit={submit} spacing={2}>
      {error && <Alert severity="error">{error}</Alert>}
      <TextField
        label="Email"
        name="email"
        type="email"
        autoComplete="email"
        required
        slotProps={{ htmlInput: { maxLength: 128 } }}
      />
      <TextField
        label="Username"
        name="username"
        autoComplete="nickname"
        required
        value={username}
        onChange={(event) => setUsername(event.target.value)}
        error={availability === "taken"}
        helperText={availability ? availabilityMessages[availability] : usernameHelp}
        slotProps={{
          htmlInput: { pattern: usernamePattern, maxLength: 24, autoCapitalize: "none" },
          formHelperText: { "aria-live": "polite" },
        }}
      />
      <TextField
        label="Password"
        name="password"
        type="password"
        autoComplete="new-password"
        helperText={passwordHelp}
        required
      />
      <TextField
        label="Confirm password"
        name="confirm_password"
        type="password"
        autoComplete="new-password"
        required
      />
      <Button type="submit" variant="contained" disabled={pending || availability === "taken"}>
        Create account
      </Button>
      <Button onClick={onBack} disabled={pending}>
        Back to sign in
      </Button>
    </Stack>
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
