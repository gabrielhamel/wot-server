import { Alert, Button, Stack, TextField, Typography } from "@mui/material";
import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { errorMessage, fieldError } from "../../../platform/infrastructure/web/http";
import { checkUsername, registerPlayer } from "../../infrastructure/admin-api";

const usernamePattern = "[A-Za-z0-9_]{3,24}";
const usernameHelp = "Use 3 to 24 letters, numbers or underscores.";
const emailHelp = "Enter a valid email address of up to 128 characters.";
const emailAtom = /^[A-Za-z0-9!#$%&'*+\-/=?^`{|}~]+$/;
const domainLabel = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/;
const passwordHelp =
  "Use 8 to 128 characters, including at least one digit, one uppercase letter, one lowercase letter, and one symbol.";
const passwordControlHelp = "Control characters are not allowed.";

function validEmail(email: string) {
  const [local, domain, extra] = email.split("@");
  return (
    email.length <= 128 &&
    !email.startsWith("{") &&
    !/\s/.test(email) &&
    extra === undefined &&
    local !== undefined &&
    local.length <= 64 &&
    local.split(".").every((part) => emailAtom.test(part)) &&
    domain !== undefined &&
    domain.includes(".") &&
    domain.split(".").every((label) => domainLabel.test(label))
  );
}

function validPassword(password: string) {
  const length = Array.from(password).length;
  return (
    length >= 8 &&
    length <= 128 &&
    !/\p{Cc}/u.test(password) &&
    /\p{Nd}/u.test(password) &&
    /\p{Lu}/u.test(password) &&
    /\p{Ll}/u.test(password) &&
    /[\p{P}\p{S}]/u.test(password)
  );
}

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
  const [emailError, setEmailError] = useState("");
  const [passwordError, setPasswordError] = useState("");
  const emailInput = useRef<HTMLInputElement>(null);
  const availability = useUsernameAvailability(username);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (pending || availability === "taken") return;
    const data = new FormData(event.currentTarget);
    const email = String(data.get("email") ?? "");
    const password = String(data.get("password") ?? "");
    const confirm = String(data.get("confirm_password") ?? "");
    setError("");
    setEmailError("");
    setPasswordError("");
    if (!validEmail(email)) {
      setEmailError(emailHelp);
      emailInput.current?.focus();
      return;
    }
    if (!validPassword(password)) {
      setPasswordError(/\p{Cc}/u.test(password) ? passwordControlHelp : passwordHelp);
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
      const emailFailure = fieldError(failure, "email");
      const passwordFailure = fieldError(failure, "password");
      if (emailFailure) {
        setEmailError(emailFailure);
        emailInput.current?.focus();
      } else if (passwordFailure) {
        setPasswordError(passwordFailure);
      } else {
        setError(errorMessage(failure, "Could not create your account."));
      }
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
        inputRef={emailInput}
        error={Boolean(emailError)}
        helperText={emailError || " "}
        onBlur={(event) => {
          if (event.target.value && !validEmail(event.target.value)) setEmailError(emailHelp);
        }}
        onChange={(event) => {
          if (emailError) setEmailError(validEmail(event.target.value) ? "" : emailHelp);
        }}
        onInvalid={(event) => {
          event.preventDefault();
          setEmailError(emailHelp);
          emailInput.current?.focus();
        }}
        required
        slotProps={{
          htmlInput: { maxLength: 128, readOnly: pending },
          formHelperText: { "aria-live": "polite" },
        }}
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
        error={Boolean(passwordError)}
        helperText={passwordError || passwordHelp}
        onChange={() => setPasswordError("")}
        slotProps={{
          htmlInput: { readOnly: pending },
          formHelperText: { "aria-live": "polite" },
        }}
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
