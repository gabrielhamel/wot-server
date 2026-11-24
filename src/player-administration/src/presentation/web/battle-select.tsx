import { TextField } from "@mui/material";
import type { BattleChoice } from "../../application/admin-contracts";

export function ChoiceSelect({
  label,
  value,
  choices,
  placeholder,
  onChange,
}: {
  label: string;
  value: number | "";
  choices: BattleChoice[];
  placeholder?: string;
  onChange: (value: number | "") => void;
}) {
  return (
    <TextField
      fullWidth
      select
      label={label}
      value={value}
      onChange={(event) => onChange(event.target.value === "" ? "" : Number(event.target.value))}
      slotProps={{ select: { native: true }, inputLabel: { shrink: true } }}
    >
      {placeholder && <option value="">{placeholder}</option>}
      {choices.map(({ id, name }) => (
        <option key={id} value={id}>
          {name}
        </option>
      ))}
    </TextField>
  );
}
