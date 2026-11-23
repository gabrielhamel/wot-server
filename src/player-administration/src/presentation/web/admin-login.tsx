import ShieldOutlined from "@mui/icons-material/ShieldOutlined";
import { Alert, Box, Button, CircularProgress, Typography } from "@mui/material";
import { useEffect, useState } from "react";
import { Form, Login, PasswordInput, TextInput, required, useLogin } from "react-admin";
import type { FieldValues } from "react-hook-form";
import { useFormState } from "react-hook-form";
import {
  errorMessage,
  fieldError,
  responseStatus,
} from "../../../../platform/src/infrastructure/web/http";
import { createFirstAdmin, getSession } from "../../infrastructure/admin-api";
import {
  passwordHelp,
  registrationEmailError,
  registrationPasswordError,
} from "./account-credential-rules";
import { RegistrationConfirmation, RegistrationForm } from "./registration";

function SubmitButton({ label }: { label: string }) {
  const { isSubmitting } = useFormState();
  return (
    <Button type="submit" variant="contained" disabled={isSubmitting} fullWidth sx={{ mt: 2 }}>
      {label}
    </Button>
  );
}

export function AdminLogin() {
  const login = useLogin();
  const [mode, setMode] = useState<"loading" | "setup" | "login" | "unavailable">("loading");
  const [attempt, setAttempt] = useState(0);
  const [error, setError] = useState("");
  const [created, setCreated] = useState(false);
  const [registrationOpen, setRegistrationOpen] = useState(false);
  const [screen, setScreen] = useState<"login" | "registration" | "confirmation">("login");
  const [registeredEmail, setRegisteredEmail] = useState("");

  useEffect(() => {
    const controller = new AbortController();
    getSession(controller.signal).then(
      (session) => {
        if (!controller.signal.aborted) {
          setMode(session.status === "setup_required" ? "setup" : "login");
          setRegistrationOpen(session.status === "signed_out" && session.registration_open);
        }
      },
      (failure) => {
        if (!controller.signal.aborted) {
          setError(errorMessage(failure));
          setMode("unavailable");
        }
      },
    );
    return () => controller.abort();
  }, [attempt]);

  const settingUp = mode === "setup";

  async function submit(values: FieldValues) {
    setError("");
    try {
      if (settingUp) {
        await createFirstAdmin({
          username: values.email,
          display_name: values.display_name,
          password: values.password,
        });
        setCreated(true);
        setRegistrationOpen(true);
        setMode("login");
      } else {
        await login({ email: values.email, password: values.password });
      }
    } catch (failure) {
      if (settingUp) {
        const email = fieldError(failure, "username");
        const displayName = fieldError(failure, "display_name");
        const password = fieldError(failure, "password");
        if (email || displayName || password) {
          return {
            ...(email && { email }),
            ...(displayName && { display_name: displayName }),
            ...(password && { password }),
          };
        }
        if (responseStatus(failure) === 409) setAttempt(attempt + 1);
      }
      setError(
        errorMessage(
          failure,
          settingUp ? "Could not create the administrator." : "Could not sign in.",
        ),
      );
    }
  }

  const title = settingUp
    ? "Create the first administrator"
    : screen === "registration"
      ? "Create a player account"
      : screen === "confirmation"
        ? "Account created"
        : "World of Tanks Server";

  return (
    <Login
      avatarIcon={<ShieldOutlined />}
      sx={{
        backgroundImage: "radial-gradient(ellipse at top, #27323f, #11161d 60%)",
        "& .RaLogin-card": { width: "min(400px, calc(100vw - 32px))", minWidth: 0 },
        "& .RaLogin-icon": { bgcolor: "primary.main", color: "primary.contrastText" },
      }}
    >
      <Box sx={{ px: 3, pb: 3 }}>
        <Typography component="h1" variant="h5" sx={{ mb: 2, textAlign: "center" }}>
          {title}
        </Typography>
        {created && <Alert severity="success">Administrator created. Sign in to continue.</Alert>}
        {error && <Alert severity="error">{error}</Alert>}
        {mode === "loading" && <CircularProgress size={24} aria-label="Checking admin access" />}
        {mode === "unavailable" && (
          <Button
            onClick={() => {
              setError("");
              setMode("loading");
              setAttempt(attempt + 1);
            }}
          >
            Retry
          </Button>
        )}
        {(settingUp || (mode === "login" && screen === "login")) && (
          <Form
            key={mode}
            onSubmit={submit}
            mode={settingUp ? "onTouched" : "onSubmit"}
            noValidate={settingUp}
            disableInvalidFormNotification={settingUp}
          >
            <TextInput
              source="email"
              label="Email"
              type={settingUp ? "email" : "text"}
              slotProps={{
                htmlInput: { inputMode: "email", maxLength: settingUp ? 128 : undefined },
                formHelperText: { "aria-live": "polite" },
              }}
              autoComplete={settingUp ? "email" : "username"}
              helperText={settingUp ? " " : false}
              validate={settingUp ? registrationEmailError : required()}
              isRequired
              fullWidth
            />
            {settingUp && (
              <TextInput
                source="display_name"
                label="Display name"
                helperText={false}
                validate={required()}
                fullWidth
              />
            )}
            <PasswordInput
              source="password"
              label="Password"
              autoComplete={settingUp ? "new-password" : "current-password"}
              helperText={settingUp ? passwordHelp : false}
              validate={settingUp ? registrationPasswordError : required()}
              isRequired
              fullWidth
            />
            <SubmitButton label={settingUp ? "Create administrator" : "Sign in"} />
          </Form>
        )}
        {mode === "login" && registrationOpen && screen === "login" && (
          <Button
            fullWidth
            sx={{ mt: 1 }}
            onClick={() => {
              setError("");
              setCreated(false);
              setScreen("registration");
            }}
          >
            Create a player account
          </Button>
        )}
        {screen === "registration" && (
          <RegistrationForm
            onBack={() => setScreen("login")}
            onComplete={(email) => {
              setRegisteredEmail(email);
              setScreen("confirmation");
            }}
          />
        )}
        {screen === "confirmation" && (
          <RegistrationConfirmation email={registeredEmail} onBack={() => setScreen("login")} />
        )}
      </Box>
    </Login>
  );
}
