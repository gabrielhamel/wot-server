import { Alert, Button, Stack, TextField } from "@mui/material";
import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { errorMessage, fieldError } from "../../../platform/infrastructure/web/http";
import type { PlayerCredentials } from "../../application/admin-contracts";
import { checkUsername } from "../../infrastructure/admin-api";

const usernameCharacters = "A-Za-z0-9_";
const minUsernameLength = 3;
const maxUsernameLength = 24;
const usernamePattern = `[${usernameCharacters}]{${minUsernameLength},${maxUsernameLength}}`;
const invalidUsernameCharacter = new RegExp(`[^${usernameCharacters}]`);
const usernameCharacterHelp = "Use only English letters (A–Z), digits, and underscores.";
const usernameLengthHelp = `Use ${minUsernameLength} to ${maxUsernameLength} characters.`;
const usernameHelp = `${usernameLengthHelp} ${usernameCharacterHelp}`;
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

function usernameFormatError(username: string) {
  if (invalidUsernameCharacter.test(username)) return usernameCharacterHelp;
  if (username.length < minUsernameLength || username.length > maxUsernameLength)
    return usernameLengthHelp;
  return "";
}

type Availability = "checking" | "available" | "taken" | "unconfirmed";
const availabilityMessages: Record<Availability, string> = {
  checking: "Checking availability…",
  available: "Username available.",
  taken: "That username is already taken.",
  unconfirmed: "Availability will be checked when you submit.",
};

function useUsernameAvailability(username: string, valid: boolean) {
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

function generatePassword() {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@";
  const bytes = new Uint8Array(24);
  let value: string;
  do {
    crypto.getRandomValues(bytes);
    value = Array.from(bytes, (byte) => alphabet.charAt(byte % alphabet.length)).join("");
  } while (!validPassword(value));
  return value;
}

export function PlayerAccountForm({
  mode,
  onCreate,
  onBack,
}: {
  mode: "registration" | "managed";
  onCreate: (credentials: PlayerCredentials) => Promise<void>;
  onBack: () => void;
}) {
  const managed = mode === "managed";
  const [usernameError, setUsernameError] = useState("");
  const [username, setUsername] = useState("");
  const [usernameTouched, setUsernameTouched] = useState(false);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [emailError, setEmailError] = useState("");
  const [passwordError, setPasswordError] = useState("");
  const emailInput = useRef<HTMLInputElement>(null);
  const usernameInput = useRef<HTMLInputElement>(null);
  const formatError = usernameFormatError(username);
  const availability = useUsernameAvailability(username, !formatError);
  const usernameMessage =
    usernameError || (usernameTouched || username.length >= minUsernameLength ? formatError : "");

  function showInvalidUsername() {
    setUsernameTouched(true);
    usernameInput.current?.focus();
  }

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
    setUsernameError("");
    if (!validEmail(email)) {
      setEmailError(emailHelp);
      emailInput.current?.focus();
      return;
    }
    if (formatError) {
      showInvalidUsername();
      return;
    }
    if (!managed && !validPassword(password)) {
      setPasswordError(/\p{Cc}/u.test(password) ? passwordControlHelp : passwordHelp);
      return;
    }
    if (!managed && password !== confirm) {
      setError("The passwords do not match.");
      return;
    }

    setPending(true);
    try {
      await onCreate({ email, username, password: managed ? generatePassword() : password });
    } catch (failure) {
      const emailFailure = fieldError(failure, managed ? "username" : "email");
      const usernameFailure = fieldError(failure, managed ? "display_name" : "username");
      const passwordFailure = fieldError(failure, "password");
      if (emailFailure) {
        setEmailError(emailFailure);
        emailInput.current?.focus();
      } else if (usernameFailure) {
        setUsernameError(usernameFailure);
      } else if (passwordFailure) {
        setPasswordError(passwordFailure);
      } else {
        setError(
          errorMessage(
            failure,
            managed ? "Could not create the player." : "Could not create your account.",
          ),
        );
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
        autoComplete={managed ? "off" : "email"}
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
        inputRef={usernameInput}
        autoComplete="nickname"
        required
        value={username}
        onChange={(event) => {
          setUsernameError("");
          setUsername(event.target.value);
        }}
        onBlur={() => setUsernameTouched(true)}
        onInvalid={(event) => {
          event.preventDefault();
          showInvalidUsername();
        }}
        error={Boolean(usernameMessage) || availability === "taken"}
        helperText={
          usernameMessage || (availability ? availabilityMessages[availability] : usernameHelp)
        }
        slotProps={{
          htmlInput: {
            pattern: usernamePattern,
            autoCapitalize: "none",
            readOnly: pending,
          },
          formHelperText: { "aria-live": "polite" },
        }}
      />
      {managed ? (
        passwordError && <Alert severity="error">{passwordError}</Alert>
      ) : (
        <>
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
        </>
      )}
      <Button type="submit" variant="contained" disabled={pending || availability === "taken"}>
        {managed ? "Create player" : "Create account"}
      </Button>
      <Button onClick={onBack} disabled={pending}>
        {managed ? "Back to players" : "Back to sign in"}
      </Button>
    </Stack>
  );
}
