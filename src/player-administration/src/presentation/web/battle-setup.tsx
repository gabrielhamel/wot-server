import {
  Alert,
  Autocomplete,
  Box,
  Button,
  CircularProgress,
  Paper,
  Stack,
  TextField,
  Typography,
} from "@mui/material";
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { Title } from "react-admin";
import type {
  AdminBattleSetup,
  BattleOptions,
  CatalogVehicle,
  DummyLoadout,
} from "../../application/admin-contracts";
import { getBattleOptions, startBattle } from "../../infrastructure/admin-api";
import { errorMessage } from "../../../../shared-infrastructure/src/infrastructure/web/http";
import { BattleParameterEditor } from "./battle-parameters";
import { ChoiceSelect } from "./battle-select";
import { DummyConfiguration } from "./dummy-configuration";

const MAX_ENTRANTS = 30;
const TEAM_OPTIONS = [
  { id: 1, name: "Team 1" },
  { id: 2, name: "Team 2" },
];
const MIN_DURATION_MINUTES = 5;
const MAX_DURATION_MINUTES = 30;

const OBSERVER = 0;
type Player = Omit<AdminBattleSetup["players"][number], "inventory_id"> & {
  vehicle: number | "";
};
type Dummy = { key: number; team: number; loadout: DummyLoadout };

function RosterCard({
  title,
  children,
  remove,
}: {
  title: string;
  children: ReactNode;
  remove: () => void;
}) {
  return (
    <Paper component="section" variant="outlined" sx={{ p: 2 }}>
      <Stack spacing={2}>
        <Stack direction="row" justifyContent="space-between" alignItems="center">
          <Typography component="h3" variant="h6">
            {title}
          </Typography>
          <Button aria-label={`Remove ${title}`} onClick={remove}>
            Remove
          </Button>
        </Stack>
        {children}
      </Stack>
    </Paper>
  );
}

function emptyLoadout(vehicle = 0): DummyLoadout {
  return { vehicle, equipment: [], consumables: [], crew_perks: [] };
}

function isReady(options: BattleOptions, map: number | "", players: Player[], dummies: Dummy[]) {
  return (
    options.maps.some(({ id }) => id === map) &&
    players.length > 0 &&
    players.every((player) =>
      options.players.some(
        ({ id, vehicles, unavailable_reason }) =>
          id === player.account_id &&
          !unavailable_reason &&
          (player.vehicle === OBSERVER
            ? options.observer_available
            : vehicles.some(
                ({ id, unavailable_reason }) => id === player.vehicle && !unavailable_reason,
              )),
      ),
    ) &&
    dummies.every(({ loadout }) => loadout.vehicle !== 0) &&
    [1, 2].every(
      (team) =>
        players.some(
          (player) => player.team === team && player.vehicle !== OBSERVER && player.vehicle !== "",
        ) || dummies.some((dummy) => dummy.team === team),
    )
  );
}

export function BattleSetup() {
  const [options, setOptions] = useState<BattleOptions>();
  const [map, setMap] = useState<number | "">("");
  const [duration, setDuration] = useState("15");
  const [parameterDrafts, setParameterDrafts] = useState<Record<string, string>>({});
  const [players, setPlayers] = useState<Player[]>([]);
  const [playerSearch, setPlayerSearch] = useState("");
  const [dummies, setDummies] = useState<Dummy[]>([]);
  const [configuring, setConfiguring] = useState<number>();
  const [attempt, setAttempt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [receipt, setReceipt] = useState<{ joined: number; requested: number }>();
  const nextDummy = useRef(0);

  useEffect(() => {
    const controller = new AbortController();
    getBattleOptions(controller.signal).then(
      (data) => {
        if (controller.signal.aborted) return;
        setOptions(data);
        setLoading(false);
      },
      (failure) => {
        if (controller.signal.aborted) return;
        setError(errorMessage(failure, "Could not load battle options."));
        setLoading(false);
      },
    );
    return () => controller.abort();
  }, [attempt]);

  function refresh() {
    setError("");
    setLoading(true);
    setAttempt((value) => value + 1);
  }

  function updatePlayer(id: number, edit: Partial<Player>) {
    setPlayers((roster) =>
      roster.map((player) => (player.account_id === id ? { ...player, ...edit } : player)),
    );
  }

  function updateDummy(key: number, edit: Partial<Dummy>) {
    setDummies((roster) =>
      roster.map((dummy) => (dummy.key === key ? { ...dummy, ...edit } : dummy)),
    );
  }

  const parameterFields = (options?.parameters ?? []).map((definition) => {
    const value = parameterDrafts[definition.key] ?? String(definition.default);
    const number = Number(value);
    const invalid =
      value === "" ||
      !Number.isFinite(number) ||
      number < definition.min ||
      number > definition.max ||
      (definition.integer && !Number.isInteger(number));
    return { ...definition, value, invalid };
  });
  const minutes = Number(duration);
  const validDuration =
    Number.isInteger(minutes) && minutes >= MIN_DURATION_MINUTES && minutes <= MAX_DURATION_MINUTES;
  const canLaunch =
    map !== "" &&
    validDuration &&
    !parameterFields.some(({ invalid }) => invalid) &&
    !saving &&
    !loading &&
    options !== undefined &&
    isReady(options, map, players, dummies);

  async function launch() {
    if (!canLaunch) return;
    setSaving(true);
    setError("");
    setReceipt(undefined);
    try {
      const parameters = Object.fromEntries(
        parameterFields
          .filter((field) => Number(field.value) !== field.default)
          .map(({ key, value }) => [key, Number(value)]),
      );
      const started = await startBattle({
        arena_type: map,
        duration_seconds: minutes * 60,
        parameters,
        players: players.map(({ vehicle, ...player }) => ({
          ...player,
          inventory_id: vehicle === OBSERVER ? null : Number(vehicle),
        })),
        dummies: dummies.map(({ team, loadout }) => ({ team, loadout })),
      });
      setReceipt({ joined: started.players_handed_off, requested: players.length });
      setPlayers([]);
      refresh();
    } catch (failure) {
      setError(errorMessage(failure, "The battle could not be started."));
    } finally {
      setSaving(false);
    }
  }

  const selectedDummyIndex = dummies.findIndex(({ key }) => key === configuring);
  const selectedDummy = dummies[selectedDummyIndex];
  const full = players.length + dummies.length >= MAX_ENTRANTS;
  return (
    <Stack spacing={2}>
      <Title title="Start battle" />
      {error && <Alert severity="error">{error}</Alert>}
      {receipt &&
        (receipt.joined === receipt.requested ? (
          <Alert severity="success">Battle started.</Alert>
        ) : (
          <Alert severity="warning">
            Battle started. {receipt.joined} of {receipt.requested} players were sent to the arena.
          </Alert>
        ))}
      {loading && <CircularProgress size={24} aria-label="Loading battle options" />}
      {!options && !loading && <Button onClick={refresh}>Retry</Button>}
      {options && options.maps.length === 0 && (
        <Alert severity="warning">Training battles are unavailable on this server.</Alert>
      )}
      {options && options.maps.length > 0 && (
        <Box
          component="fieldset"
          disabled={saving || loading}
          sx={{ m: 0, p: 0, border: 0, minWidth: 0 }}
        >
          <Stack spacing={3}>
            <Stack direction={{ xs: "column", sm: "row" }} spacing={2}>
              <ChoiceSelect
                label="Map and battle type"
                value={map}
                choices={options.maps}
                placeholder="Select a map"
                onChange={setMap}
              />
              <TextField
                fullWidth
                label="Battle duration (minutes)"
                type="number"
                value={duration}
                error={!validDuration}
                helperText={validDuration ? undefined : "Enter a whole number from 5 to 30."}
                slotProps={{
                  htmlInput: { min: MIN_DURATION_MINUTES, max: MAX_DURATION_MINUTES, step: 1 },
                }}
                onChange={(event) => setDuration(event.target.value)}
              />
            </Stack>
            {parameterFields.length > 0 && (
              <BattleParameterEditor
                fields={parameterFields}
                onChange={(key, value) =>
                  setParameterDrafts((drafts) => ({ ...drafts, [key]: value }))
                }
                onReset={() => setParameterDrafts({})}
              />
            )}
            <Stack spacing={2}>
              <Stack direction="row" justifyContent="space-between" alignItems="center">
                <Typography component="h3" variant="h6">
                  Players
                </Typography>
                <Button onClick={refresh}>Refresh players</Button>
              </Stack>
              {options.players.length === 0 && (
                <Alert severity="info">
                  Players appear here when connected and available in the garage.
                </Alert>
              )}
              <Autocomplete
                disabled={full || loading || saving}
                value={null}
                inputValue={playerSearch}
                onInputChange={(_, value, reason) => {
                  if (reason === "input" || reason === "clear") setPlayerSearch(value);
                }}
                options={options.players.filter(
                  ({ id }) => !players.some(({ account_id }) => id === account_id),
                )}
                getOptionLabel={({ name, unavailable_reason }) =>
                  unavailable_reason ? `${name} · ${unavailable_reason}` : name
                }
                getOptionDisabled={({ unavailable_reason }) => Boolean(unavailable_reason)}
                getOptionKey={({ id }) => id}
                onChange={(_, player) => {
                  if (player) {
                    setPlayerSearch("");
                    setPlayers([
                      ...players,
                      {
                        account_id: player.id,
                        team: 1,
                        vehicle:
                          player.vehicles.find((vehicle) => !vehicle.unavailable_reason)?.id ?? "",
                      },
                    ]);
                  }
                }}
                renderInput={(params) => <TextField {...params} label="Add connected player" />}
              />
              {players.map((player) => {
                const available = options.players.find(({ id }) => id === player.account_id);
                const name = available?.name ?? "Unavailable player";
                const playerUnavailable =
                  available?.unavailable_reason ??
                  (available ? null : "This player is no longer available.");
                const vehicles = available?.vehicles ?? [];
                const choices = options.observer_available
                  ? [...vehicles, { id: OBSERVER, name: "Observer" }]
                  : vehicles;
                return (
                  <RosterCard
                    key={player.account_id}
                    title={name}
                    remove={() => setPlayers(players.filter((entry) => entry !== player))}
                  >
                    {playerUnavailable && <Alert severity="warning">{playerUnavailable}</Alert>}
                    <Stack direction={{ xs: "column", sm: "row" }} spacing={2}>
                      <ChoiceSelect
                        label={`Team for ${name}`}
                        value={player.team}
                        choices={TEAM_OPTIONS}
                        onChange={(team) => {
                          if (team !== "") updatePlayer(player.account_id, { team });
                        }}
                      />
                      <ChoiceSelect
                        label={`Vehicle for ${name}`}
                        value={
                          choices.some(({ id }) => id === player.vehicle) ? player.vehicle : ""
                        }
                        choices={choices}
                        disabled={Boolean(playerUnavailable)}
                        placeholder="Select a vehicle"
                        onChange={(vehicle) => updatePlayer(player.account_id, { vehicle })}
                      />
                    </Stack>
                  </RosterCard>
                );
              })}
            </Stack>
            <Stack spacing={2}>
              <Stack direction="row" justifyContent="space-between" alignItems="center">
                <Typography component="h3" variant="h6">
                  Stationary dummies
                </Typography>
                <Button
                  disabled={full}
                  onClick={() =>
                    setDummies([
                      ...dummies,
                      { key: ++nextDummy.current, team: 2, loadout: emptyLoadout() },
                    ])
                  }
                >
                  Add dummy
                </Button>
              </Stack>
              {dummies.map(({ key, team, loadout }, index) => (
                <RosterCard
                  key={key}
                  title={`Dummy ${index + 1}`}
                  remove={() => setDummies(dummies.filter((dummy) => dummy.key !== key))}
                >
                  <Stack direction={{ xs: "column", sm: "row" }} spacing={2}>
                    <ChoiceSelect
                      label={`Team for Dummy ${index + 1}`}
                      value={team}
                      choices={TEAM_OPTIONS}
                      onChange={(value) => {
                        if (value !== "") updateDummy(key, { team: value });
                      }}
                    />
                    <Autocomplete<CatalogVehicle>
                      fullWidth
                      value={
                        options.dummy_vehicles.find(({ id }) => id === loadout.vehicle) ?? null
                      }
                      options={options.dummy_vehicles}
                      getOptionLabel={({ name }) => name}
                      getOptionKey={({ id }) => id}
                      isOptionEqualToValue={(option, value) => option.id === value.id}
                      onChange={(_, value) =>
                        updateDummy(key, { loadout: emptyLoadout(value?.id) })
                      }
                      renderInput={(params) => (
                        <TextField
                          {...params}
                          margin="none"
                          label={`Vehicle for Dummy ${index + 1}`}
                        />
                      )}
                    />
                  </Stack>
                  <Button
                    variant="outlined"
                    disabled={!loadout.vehicle}
                    onClick={() => setConfiguring(key)}
                  >
                    Customize Dummy {index + 1}
                  </Button>
                </RosterCard>
              ))}
            </Stack>
            <Stack spacing={1} alignItems="start">
              <Typography variant="body2" color="text.secondary">
                Include a connected player and at least one vehicle on each team. Up to{" "}
                {MAX_ENTRANTS} entrants.
              </Typography>
              <Button variant="contained" disabled={!canLaunch} onClick={() => void launch()}>
                {saving ? "Starting battle…" : "Start battle"}
              </Button>
            </Stack>
          </Stack>
        </Box>
      )}
      {selectedDummy && (
        <DummyConfiguration
          key={selectedDummy.key}
          name={`Dummy ${selectedDummyIndex + 1}`}
          initial={selectedDummy.loadout}
          onClose={() => setConfiguring(undefined)}
          onSave={(loadout) => {
            updateDummy(selectedDummy.key, { loadout });
            setConfiguring(undefined);
          }}
        />
      )}
    </Stack>
  );
}
