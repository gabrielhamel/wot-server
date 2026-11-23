import { useState } from "react";
import type { FormEvent } from "react";
import {
  Alert,
  Button,
  Card,
  CardContent,
  MenuItem,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { currencies } from "../../../application/admin-contracts";
import type { Currency, Grant } from "../../../application/admin-contracts";

const maxGrantAmount = 2_147_483_647;

export function CurrencyGrant({
  busy,
  onGrant,
}: {
  busy: boolean;
  onGrant: (grant: Grant) => Promise<boolean>;
}) {
  const [currency, setCurrency] = useState<Currency>("credits");
  const [amount, setAmount] = useState("");
  const [validation, setValidation] = useState("");

  async function grantCurrency(event: FormEvent) {
    event.preventDefault();
    const value = Number(amount);
    if (!/^\d+$/.test(amount) || value < 1 || value > maxGrantAmount) {
      setValidation("Enter a whole amount from 1 to 2,147,483,647.");
      return;
    }
    if (await onGrant({ kind: "currency", currency, amount: value })) setAmount("");
  }

  return (
    <Card component="form" onSubmit={grantCurrency}>
      <CardContent component={Stack} spacing={2}>
        <Typography variant="h6" component="h2">
          Grant resources
        </Typography>
        <TextField
          select
          label="Resource"
          value={currency}
          onChange={(e) => setCurrency(e.target.value as Currency)}
          disabled={busy}
          fullWidth
        >
          {currencies.map(([key, label]) => (
            <MenuItem key={key} value={key}>
              {label}
            </MenuItem>
          ))}
        </TextField>
        <TextField
          label="Amount to add"
          value={amount}
          onChange={(e) => {
            setAmount(e.target.value);
            setValidation("");
          }}
          slotProps={{ htmlInput: { inputMode: "numeric", maxLength: 10 } }}
          required
          disabled={busy}
          fullWidth
        />
        {validation && <Alert severity="error">{validation}</Alert>}
        <Button
          type="submit"
          variant="contained"
          disabled={busy || !amount}
          sx={{ alignSelf: "start" }}
        >
          Grant resource
        </Button>
      </CardContent>
    </Card>
  );
}
