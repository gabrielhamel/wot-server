import ShieldOutlined from "@mui/icons-material/ShieldOutlined";
import { Box, Button, Typography } from "@mui/material";
import { Form, Login, PasswordInput, required, useLogin, useNotify } from "react-admin";
import type { FieldValues } from "react-hook-form";
import { useFormState } from "react-hook-form";
import { errorMessage } from "../../../platform/infrastructure/web/http";

function SignInButton() {
  const { isSubmitting } = useFormState();
  return (
    <Button type="submit" variant="contained" disabled={isSubmitting} fullWidth sx={{ mt: 2 }}>
      Sign in
    </Button>
  );
}

export function TokenLogin() {
  const login = useLogin();
  const notify = useNotify();

  async function signIn(values: FieldValues) {
    try {
      await login(values);
    } catch (error) {
      notify(errorMessage(error, "Could not sign in."), { type: "error" });
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
          World of Tanks Server
        </Typography>
        <Form onSubmit={signIn}>
          <PasswordInput
            source="token"
            label="Admin token"
            helperText={false}
            validate={required()}
            fullWidth
          />
          <SignInButton />
        </Form>
      </Box>
    </Login>
  );
}
