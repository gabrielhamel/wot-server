import ShieldOutlined from "@mui/icons-material/ShieldOutlined";
import { Alert, Box, Button, CircularProgress, Typography } from "@mui/material";
import { useEffect, useState } from "react";
import { Form, Login, PasswordInput, TextInput, required, useLogin } from "react-admin";
import type { FieldValues } from "react-hook-form";
import { useFormState } from "react-hook-form";
import { errorMessage, responseStatus } from "../../../platform/infrastructure/web/http";
import { createFirstAdmin, getSession } from "../../infrastructure/admin-api";

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

  useEffect(() => {
    const controller = new AbortController();
    getSession(controller.signal).then(
      (session) => {
        if (!controller.signal.aborted) {
          setMode(session.status === "setup_required" ? "setup" : "login");
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

  async function submit(values: FieldValues) {
    setError("");
    try {
      if (mode === "setup") {
        await createFirstAdmin({
          username: values.email,
          display_name: values.display_name,
          password: values.password,
        });
        setCreated(true);
        setMode("login");
      } else {
        await login({ email: values.email, password: values.password });
      }
    } catch (failure) {
      setError(errorMessage(failure, "Could not sign in."));
      if (mode === "setup" && responseStatus(failure) === 409) setMode("login");
    }
  }

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
          {mode === "setup" ? "Create the first administrator" : "World of Tanks Server"}
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
        {(mode === "setup" || mode === "login") && (
          <Form key={mode} onSubmit={submit}>
            <TextInput
              source="email"
              label="Email"
              slotProps={{ htmlInput: { inputMode: "email" } }}
              autoComplete="username"
              helperText={false}
              validate={required()}
              fullWidth
            />
            {mode === "setup" && (
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
              autoComplete={mode === "setup" ? "new-password" : "current-password"}
              helperText={false}
              validate={required()}
              fullWidth
            />
            <SubmitButton label={mode === "setup" ? "Create administrator" : "Sign in"} />
          </Form>
        )}
      </Box>
    </Login>
  );
}
