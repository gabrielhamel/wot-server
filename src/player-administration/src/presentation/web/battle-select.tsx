import { TextField } from "@mui/material";
import type { BattleChoice } from "../../application/admin-contracts";

export function ChoiceSelect({
  label,
  value,
  choices,
  placeholder,
  disabled,
  onChange,
}: {
  label: string;
  value: number | "";
  choices: BattleChoice[];
  placeholder?: string;
  disabled?: boolean;
  onChange: (value: number | "") => void;
}) {
  return (
    <TextField
      fullWidth
      select
      label={label}
      disabled={disabled}
      value={value}
      onChange={(event) => onChange(event.target.value === "" ? "" : Number(event.target.value))}
      slotProps={{ select: { native: true }, inputLabel: { shrink: true } }}
    >
      {placeholder && <option value="">{placeholder}</option>}
      {choices.map(({ id, name, unavailable_reason }) => (
        <option key={id} value={id} disabled={Boolean(unavailable_reason)}>
          {unavailable_reason ? `${name} · ${unavailable_reason}` : name}
        </option>
      ))}
    </TextField>
  );
}
