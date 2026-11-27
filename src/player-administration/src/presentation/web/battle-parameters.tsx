import ExpandMoreIcon from "@mui/icons-material/ExpandMore";
import {
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

import { ChoiceSelect } from "./battle-select";

export type BattleParameterField = BattleParameterDefinition & {
  value: string;
  invalid: boolean;
};

export function BattleParameterEditor({
  fields,
  onChange,
  onReset,
}: {
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
      defaultExpanded={false}
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
                {entries.map(({ key, label, min, max, integer, choices, value, invalid }) =>
                  choices ? (
                    <ChoiceSelect
                      key={key}
                      label={label}
                      value={Number(value)}
                      choices={choices.map((name, id) => ({ id, name }))}
                      onChange={(selected) => onChange(key, String(selected))}
                    />
                  ) : (
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
                  ),
                )}
              </Box>
            </Stack>
          ))}
          <Button onClick={onReset} sx={{ alignSelf: "start" }}>
            Reset parameters to defaults
          </Button>
        </Stack>
      </AccordionDetails>
    </Accordion>
  );
}
