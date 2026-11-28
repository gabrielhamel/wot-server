import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import {
  Alert,
  Accordion,
  AccordionDetails,
  AccordionSummary,
  Box,
  Button,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { useId } from "react";
import type { BattleParameterDefinition } from "../../application/admin-contracts";

export type BattleParameterField = BattleParameterDefinition & {
  value: string;
  invalid: boolean;
};

export function BattleParameterEditor({
  expanded,
  onToggle,
  status,
  warnings,
  fields,
  onChange,
  onReset,
}: {
  expanded: boolean;
  onToggle: (expanded: boolean) => void;
  status: string;
  warnings: string[];
  fields: BattleParameterField[];
  onChange: (key: string, value: string) => void;
  onReset: () => void;
}) {
  const id = useId();
  const groups = new Map<string, BattleParameterField[]>();
  for (const field of fields) {
    const group = groups.get(field.group);
    if (group) group.push(field);
    else groups.set(field.group, [field]);
  }

  return (
    <Accordion
      expanded={expanded}
      onChange={(_, open) => onToggle(open)}
      variant="outlined"
      slotProps={{ transition: { unmountOnExit: true } }}
    >
      <AccordionSummary id={id} aria-controls={`${id}-fields`} expandIcon={<ExpandMoreIcon />}>
        <Typography component="span" variant="subtitle1">
          Battle parameters
        </Typography>
      </AccordionSummary>
      <AccordionDetails>
        <Stack spacing={3}>
          <Stack spacing={1} alignItems="start">
            {status && <Alert severity="info">{status}</Alert>}
            {warnings.map((message) => (
              <Alert key={message} severity="warning">
                {message}
              </Alert>
            ))}
            <Button variant="outlined" onClick={onReset}>
              Reset battle parameters
            </Button>
          </Stack>
          {Array.from(groups, ([group, entries]) => (
            <Stack key={group} spacing={1}>
              <Typography component="h3" variant="subtitle1">
                {group}
              </Typography>
              <Box
                sx={{
                  display: "grid",
                  gap: 2,
                  gridTemplateColumns: { xs: "minmax(0, 1fr)", md: "repeat(2, minmax(0, 1fr))" },
                  pt: 1,
                }}
              >
                {entries.map(({ key, label, min, max, integer, value, invalid }) => (
                  <TextField
                    key={key}
                    label={label}
                    type="number"
                    value={value}
                    error={invalid}
                    helperText={
                      invalid
                        ? `Enter ${integer ? "a whole number" : "a number"} from ${min} to ${max}.`
                        : undefined
                    }
                    slotProps={{ htmlInput: { min, max, step: integer ? 1 : "any" } }}
                    onChange={(event) => onChange(key, event.target.value)}
                  />
                ))}
              </Box>
            </Stack>
          ))}
        </Stack>
      </AccordionDetails>
    </Accordion>
  );
}
