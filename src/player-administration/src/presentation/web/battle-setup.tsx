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
import { useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { Title, useLocation, useNavigate } from "react-admin";
import type {
  AdminBattleSetup,
  BattleOptions,
  CatalogVehicle,
  CustomBattleLoadout,
} from "../../application/admin-contracts";
import { getBattleOptions, startBattle } from "../../infrastructure/admin-api";
import { errorMessage } from "../../../../shared-infrastructure/src/infrastructure/web/http";
import { BattleParameterEditor } from "./battle-parameters";
import { ChoiceSelect } from "./battle-select";
import { BattleLoadoutConfiguration } from "./battle-loadout-configuration";
import { MAX_ENTRANTS, readBattleDraft, updateBattleQuery } from "./battle-setup-query";
import type { BattleDraft, Dummy, Player } from "./battle-setup-query";

const TEAM_OPTIONS = [
  { id: 1, name: "Team 1" },
  { id: 2, name: "Team 2" },
];
const MIN_DURATION_MINUTES = 5;
const MAX_DURATION_MINUTES = 30;
const OBSERVER = 0;

type ConfigureTarget = { kind: "owned" | "custom" | "dummy"; id: number };

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

function CatalogVehiclePicker({
  label,
  vehicles,
  vehicle,
  disabled,
  onChange,
}: {
  label: string;
  vehicles: CatalogVehicle[];
  vehicle: number;
  disabled?: boolean;
  onChange: (id: number | undefined) => void;
}) {
  return (
    <Autocomplete<CatalogVehicle>
      fullWidth
      disabled={disabled}
      value={vehicles.find(({ id }) => id === vehicle) ?? null}
      options={vehicles}
      getOptionLabel={({ name }) => name}
      getOptionKey={({ id }) => id}
      isOptionEqualToValue={(option, value) => option.id === value.id}
      onChange={(_, value) => onChange(value?.id)}
      renderInput={(params) => <TextField {...params} margin="none" label={label} />}
    />
  );
}

function emptyLoadout(vehicle = 0): CustomBattleLoadout {
  return { vehicle, equipment: [], consumables: [], crew_perks: [] };
}

function isReady(options: BattleOptions, map: number | "", players: Player[], dummies: Dummy[]) {
  const hasCatalogVehicle = ({ vehicle }: CustomBattleLoadout) =>
    options.dummy_vehicles.some(({ id }) => id === vehicle);
  return (
    options.maps.some(({ id }) => id === map) &&
    players.length > 0 &&
    players.every((player) => {
      const available = options.players.find(({ id }) => id === player.account_id);
      if (!available || available.unavailable_reason) return false;
      if (player.vehicleSource === "custom") return hasCatalogVehicle(player.customLoadout);
      if (player.vehicle === OBSERVER) return options.observer_available;
      return available.vehicles.some(
        ({ id, unavailable_reason }) => id === player.vehicle && !unavailable_reason,
      );
    }) &&
    dummies.every(({ loadout }) => hasCatalogVehicle(loadout)) &&
    [1, 2].every(
      (team) =>
        players.some(
          (player) =>
            player.team === team &&
            (player.vehicleSource === "custom" ||
              (player.vehicle !== OBSERVER && player.vehicle !== "")),
        ) || dummies.some((dummy) => dummy.team === team),
    )
  );
}

export function BattleSetup() {
  const { search } = useLocation();
  const navigate = useNavigate();
  const { map, duration, parameterDrafts, players, dummies } = useMemo(
    () => readBattleDraft(search),
    [search],
  );
  const [options, setOptions] = useState<BattleOptions>();
  const [playerSearch, setPlayerSearch] = useState("");
  const [configuring, setConfiguring] = useState<ConfigureTarget>();
  const [attempt, setAttempt] = useState(0);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [receipt, setReceipt] = useState<{ joined: number; requested: number }>();

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

  function updateDraft(changes: Partial<BattleDraft>) {
    const nextSearch = updateBattleQuery(search, changes);
    if (nextSearch !== search) navigate({ search: nextSearch }, { replace: true });
  }

  function updatePlayer(id: number, edit: Partial<Player>) {
    updateDraft({
      players: players.map((player) =>
        player.account_id === id ? { ...player, ...edit } : player,
      ),
    });
  }

  function updateDummy(key: number, edit: Partial<Dummy>) {
    updateDraft({
      dummies: dummies.map((dummy) => (dummy.key === key ? { ...dummy, ...edit } : dummy)),
    });
  }

  function addDummy() {
    let key = 1;
    while (dummies.some((dummy) => dummy.key === key)) key++;
    updateDraft({ dummies: [...dummies, { key, team: 2, loadout: emptyLoadout() }] });
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
  const entrantCount = players.length + dummies.length;
  const excessEntrants = entrantCount - MAX_ENTRANTS;
  const canLaunch =
    map !== "" &&
    validDuration &&
    excessEntrants <= 0 &&
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
      const roster: AdminBattleSetup["players"] = players.map(
        ({ account_id, team, vehicle, vehicleSource, customLoadout, ownedEdits }) => ({
          account_id,
          team,
          inventory_id: vehicleSource === "custom" || vehicle === OBSERVER ? null : Number(vehicle),
          loadout: vehicleSource === "custom" ? customLoadout : undefined,
          owned_overrides:
            vehicleSource === "owned" && vehicle !== "" && vehicle !== OBSERVER
              ? ownedEdits[vehicle]
              : undefined,
        }),
      );
      const started = await startBattle({
        arena_type: map,
        duration_seconds: minutes * 60,
        parameters,
        players: roster,
        dummies: dummies.map(({ team, loadout }) => ({ team, loadout })),
      });
      setReceipt({ joined: started.players_handed_off, requested: players.length });
      refresh();
    } catch (failure) {
      setError(errorMessage(failure, "The battle could not be started."));
    } finally {
      setSaving(false);
    }
  }

  const selectedDummyIndex =
    configuring?.kind === "dummy" ? dummies.findIndex(({ key }) => key === configuring.id) : -1;
  const selectedPlayer =
    configuring && configuring.kind !== "dummy"
      ? players.find(({ account_id }) => account_id === configuring.id)
      : undefined;
  const ownedVehicle =
    configuring?.kind === "owned" &&
    selectedPlayer &&
    selectedPlayer.vehicle !== "" &&
    selectedPlayer.vehicle !== OBSERVER
      ? { account: selectedPlayer.account_id, inventory: selectedPlayer.vehicle }
      : undefined;
  const catalogLoadout =
    configuring?.kind === "custom"
      ? selectedPlayer?.customLoadout
      : dummies[selectedDummyIndex]?.loadout;
  const selectedLoadout = ownedVehicle
    ? selectedPlayer?.ownedEdits[ownedVehicle.inventory]
    : catalogLoadout;
  const selectedName = selectedPlayer
    ? (options?.players.find(({ id }) => id === selectedPlayer.account_id)?.name ?? "Player tank")
    : `Dummy ${selectedDummyIndex + 1}`;
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
                value={options.maps.some(({ id }) => id === map) ? map : ""}
                choices={options.maps}
                placeholder="Select a map"
                onChange={(map) => updateDraft({ map })}
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
                onChange={(event) => updateDraft({ duration: event.target.value })}
              />
            </Stack>
            {parameterFields.length > 0 && (
              <BattleParameterEditor
                fields={parameterFields}
                onChange={(key, value) =>
                  updateDraft({ parameterDrafts: { ...parameterDrafts, [key]: value } })
                }
                onReset={() => updateDraft({ parameterDrafts: {} })}
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
              {excessEntrants > 0 && (
                <Alert severity="warning">
                  Remove {excessEntrants} {excessEntrants === 1 ? "entrant" : "entrants"} before
                  starting the battle. The server allows up to {MAX_ENTRANTS}.
                </Alert>
              )}
              <Autocomplete
                disabled={players.length >= MAX_ENTRANTS || loading || saving}
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
                    updateDraft({
                      players: [
                        ...players,
                        {
                          account_id: player.id,
                          team: 1,
                          vehicleSource: "owned",
                          customLoadout: emptyLoadout(),
                          ownedEdits: {},
                          vehicle:
                            player.vehicles.find((vehicle) => !vehicle.unavailable_reason)?.id ??
                            "",
                        },
                      ],
                    });
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
                const selectingCustom = player.vehicleSource === "custom";
                const editableVehicle = selectingCustom
                  ? options.dummy_vehicles.some(({ id }) => id === player.customLoadout.vehicle)
                  : vehicles.some(
                      ({ id, unavailable_reason }) => id === player.vehicle && !unavailable_reason,
                    );
                const choices = options.observer_available
                  ? [...vehicles, { id: OBSERVER, name: "Observer" }]
                  : vehicles;
                return (
                  <RosterCard
                    key={player.account_id}
                    title={name}
                    remove={() =>
                      updateDraft({ players: players.filter((entry) => entry !== player) })
                    }
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
                      {selectingCustom ? (
                        <CatalogVehiclePicker
                          label={`Custom vehicle for ${name}`}
                          vehicle={player.customLoadout.vehicle}
                          vehicles={options.dummy_vehicles}
                          disabled={Boolean(playerUnavailable)}
                          onChange={(vehicle) =>
                            updatePlayer(player.account_id, {
                              customLoadout: emptyLoadout(vehicle),
                            })
                          }
                        />
                      ) : (
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
                      )}
                    </Stack>
                    <Stack direction={{ xs: "column", sm: "row" }} spacing={1}>
                      <Button
                        aria-label={
                          selectingCustom
                            ? `Show owned tanks for ${name}`
                            : `Choose custom tank for ${name}`
                        }
                        disabled={
                          Boolean(playerUnavailable) ||
                          (!selectingCustom && options.dummy_vehicles.length === 0)
                        }
                        onClick={() =>
                          updatePlayer(player.account_id, {
                            vehicleSource: selectingCustom ? "owned" : "custom",
                          })
                        }
                      >
                        {selectingCustom ? "Show owned tanks" : "Choose custom tank"}
                      </Button>
                      <Button
                        variant="outlined"
                        aria-label={`Customize tank for ${name}`}
                        disabled={Boolean(playerUnavailable) || !editableVehicle}
                        onClick={() =>
                          setConfiguring({
                            kind: selectingCustom ? "custom" : "owned",
                            id: player.account_id,
                          })
                        }
                      >
                        Customize tank
                      </Button>
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
                <Button disabled={entrantCount >= MAX_ENTRANTS} onClick={addDummy}>
                  Add dummy
                </Button>
              </Stack>
              {dummies.map(({ key, team, loadout }, index) => (
                <RosterCard
                  key={key}
                  title={`Dummy ${index + 1}`}
                  remove={() =>
                    updateDraft({ dummies: dummies.filter((dummy) => dummy.key !== key) })
                  }
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
                    <CatalogVehiclePicker
                      label={`Vehicle for Dummy ${index + 1}`}
                      vehicle={loadout.vehicle}
                      vehicles={options.dummy_vehicles}
                      onChange={(vehicle) => updateDummy(key, { loadout: emptyLoadout(vehicle) })}
                    />
                  </Stack>
                  <Button
                    variant="outlined"
                    disabled={!options.dummy_vehicles.some(({ id }) => id === loadout.vehicle)}
                    onClick={() => setConfiguring({ kind: "dummy", id: key })}
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
      {configuring && (ownedVehicle || catalogLoadout) && (
        <BattleLoadoutConfiguration
          key={`${configuring.kind}-${configuring.id}`}
          name={selectedName}
          catalogVehicle={catalogLoadout?.vehicle}
          ownedVehicle={ownedVehicle}
          initial={selectedLoadout}
          onClose={() => setConfiguring(undefined)}
          onSave={(loadout) => {
            if (ownedVehicle && selectedPlayer) {
              updatePlayer(configuring.id, {
                ownedEdits: { ...selectedPlayer.ownedEdits, [ownedVehicle.inventory]: loadout },
              });
            } else if (configuring.kind === "custom") {
              updatePlayer(configuring.id, { customLoadout: loadout });
            } else {
              updateDummy(configuring.id, { loadout });
            }
            setConfiguring(undefined);
          }}
        />
      )}
    </Stack>
  );
}
