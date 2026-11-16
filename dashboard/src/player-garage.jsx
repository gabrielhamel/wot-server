import { useEffect, useState } from 'react';
import {
  Alert, Autocomplete, Box, Button, Card, CardContent, CircularProgress, MenuItem,
  Stack, Table, TableBody, TableCell, TableHead, TableRow, TextField, Typography,
} from '@mui/material';
import { ListButton, Title, useDataProvider, useGetRecordId } from 'react-admin';

const currencies = [
  ['credits', 'Credits'],
  ['gold', 'Gold'],
  ['bonds', 'Bonds'],
  ['free_xp', 'Free XP'],
];
const maxGrantAmount = 2_147_483_647;

export function PlayerGarage() {
  const id = useGetRecordId();
  return <GaragePage key={id} id={id} />;
}

function GaragePage({ id }) {
  const api = useDataProvider();
  const [garage, setGarage] = useState(null);
  const [catalog, setCatalog] = useState([]);
  const [loadError, setLoadError] = useState('');
  const [catalogError, setCatalogError] = useState('');
  const [busy, setBusy] = useState(false);
  const [currency, setCurrency] = useState('credits');
  const [amount, setAmount] = useState('');
  const [vehicle, setVehicle] = useState(null);
  const [feedback, setFeedback] = useState(null);
  const [refresh, setRefresh] = useState(0);

  useEffect(() => {
    const controller = new AbortController();
    api.getGarage(id, controller.signal).then(setGarage).catch(error => {
      if (!controller.signal.aborted) setLoadError(error.message);
    });
    return () => controller.abort();
  }, [api, id, refresh]);

  useEffect(() => {
    const controller = new AbortController();
    api.getVehicleCatalog(controller.signal).then(setCatalog).catch(error => {
      if (!controller.signal.aborted) setCatalogError(error.message);
    });
    return () => controller.abort();
  }, [api]);

  async function submit(body) {
    setBusy(true);
    setFeedback(null);
    try {
      const result = await api.grant(id, body);
      setGarage(result.garage);
      if (body.kind === 'currency') setAmount('');
      else setVehicle(null);
      setFeedback({
        severity: 'success',
        message: result.delivery === 'queued_for_garage'
          ? 'Grant applied. A live garage update was queued.'
          : 'Grant applied. The player will see it when the garage next syncs.',
      });
    } catch (error) {
      setFeedback({ severity: 'error', message: error.message });
    } finally {
      setBusy(false);
    }
  }

  function grantCurrency(event) {
    event.preventDefault();
    const value = Number(amount);
    if (!/^\d+$/.test(amount) || value < 1 || value > maxGrantAmount) {
      setFeedback({ severity: 'error', message: 'Enter a whole amount from 1 to 2,147,483,647.' });
      return;
    }
    void submit({ kind: 'currency', currency, amount: value });
  }

  function reload() {
    setGarage(null);
    setLoadError('');
    setRefresh(value => value + 1);
  }

  const owned = new Set(garage?.vehicles.map(tank => tank.key));
  const full = garage && garage.vehicles.length >= garage.slots;

  return (
    <Stack spacing={2} sx={{ maxWidth: 1040, pb: 3 }}>
      <Title title={garage ? `${garage.display_name} · Resources & Garage` : 'Resources & Garage'} />
      <Stack direction="row" justifyContent="space-between" alignItems="center">
        <ListButton resource="players" label="Player accounts" />
        <Button onClick={reload} disabled={busy || (!garage && !loadError)}>Refresh balances</Button>
      </Stack>
      {loadError && <Alert severity="error">{loadError}</Alert>}
      {!garage && !loadError && <CircularProgress aria-label="Loading player garage" size={24} />}
      {garage && <>
        <Card>
          <CardContent>
            <Typography variant="h6" component="h2" gutterBottom>Current balances</Typography>
            <Box component="dl" sx={{
              display: 'grid', gridTemplateColumns: { xs: 'repeat(2, 1fr)', sm: 'repeat(4, 1fr)' },
              gap: 2, m: 0,
            }}>
              {currencies.map(([key, label]) => (
                <Box key={key}>
                  <Typography component="dt" variant="body2" color="text.secondary">{label}</Typography>
                  <Typography component="dd" variant="h6" sx={{ m: 0 }}>
                    {BigInt(garage[key]).toLocaleString()}
                  </Typography>
                </Box>
              ))}
            </Box>
          </CardContent>
        </Card>

        {feedback && <Alert severity={feedback.severity} onClose={() => setFeedback(null)}>{feedback.message}</Alert>}
        <Box sx={{ display: 'grid', gridTemplateColumns: { xs: '1fr', md: '1fr 1fr' }, gap: 2 }}>
          <Card component="form" onSubmit={grantCurrency}>
            <CardContent component={Stack} spacing={2}>
              <Typography variant="h6" component="h2">Grant resources</Typography>
              <TextField select label="Resource" value={currency} onChange={e => setCurrency(e.target.value)}
                disabled={busy} fullWidth>
                {currencies.map(([key, label]) => <MenuItem key={key} value={key}>{label}</MenuItem>)}
              </TextField>
              <TextField label="Amount to add" value={amount} onChange={e => setAmount(e.target.value)}
                slotProps={{ htmlInput: { inputMode: 'numeric', maxLength: 10 } }}
                required disabled={busy} fullWidth />
              <Button type="submit" variant="contained" disabled={busy || !amount} sx={{ alignSelf: 'start' }}>
                Grant resource
              </Button>
            </CardContent>
          </Card>
          <Card component="form" onSubmit={event => {
            event.preventDefault();
            if (vehicle && !full && !owned.has(vehicle.key)) void submit({ kind: 'vehicle', vehicle: vehicle.id });
          }}>
            <CardContent component={Stack} spacing={2}>
              <Typography variant="h6" component="h2">Grant a tank</Typography>
              {catalogError && <Alert severity="error">{catalogError}</Alert>}
              {full && <Alert severity="warning">The player has no free garage slots.</Alert>}
              <Autocomplete options={catalog} value={vehicle} onChange={(_, value) => setVehicle(value)}
                getOptionKey={option => option.key} isOptionEqualToValue={(a, b) => a.id === b.id}
                getOptionLabel={option => option.name}
                getOptionDisabled={option => owned.has(option.key)}
                loading={!catalog.length && !catalogError} disabled={busy || !!full}
                renderOption={(props, option) => {
                  const { key, ...attributes } = props;
                  return <li key={key} {...attributes}>
                    <Stack>
                      <span>{option.name}{owned.has(option.key) ? ' · Owned' : ''}</span>
                      <Typography variant="caption" color="text.secondary">
                        Tier {option.tier}
                      </Typography>
                    </Stack>
                  </li>;
                }}
                renderInput={params => <TextField {...params} label="Tank" placeholder="Search by name" />} />
              <Button type="submit" variant="contained" disabled={busy || !vehicle || !!full || owned.has(vehicle.key)}
                sx={{ alignSelf: 'start' }}>
                Grant tank
              </Button>
            </CardContent>
          </Card>
        </Box>
        <Card>
          <CardContent>
            <Typography variant="h6" component="h2" gutterBottom>Owned tanks</Typography>
            <Typography variant="body2" color="text.secondary">
              {garage.vehicles.length} of {garage.slots} garage slots occupied
            </Typography>
          </CardContent>
          {garage.vehicles.length > 0 && <Table size="small">
            <TableHead>
              <TableRow><TableCell>Tank</TableCell><TableCell align="right">Tier</TableCell></TableRow>
            </TableHead>
            <TableBody>
              {garage.vehicles.map(tank => (
                <TableRow key={tank.key}>
                  <TableCell component="th" scope="row">
                    {tank.name === tank.key ? 'Unknown tank' : tank.name}
                  </TableCell>
                  <TableCell align="right">{tank.tier ?? '—'}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>}
        </Card>
      </>}
    </Stack>
  );
}
