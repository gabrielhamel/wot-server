import { useEffect, useState } from 'react';
import {
  Alert, Autocomplete, Button, Card, CardContent, Stack, TextField, Typography,
} from '@mui/material';

const maxGrantCount = 100;

export function AttachmentGrant({ api, busy, owned, onGrant }) {
  const [catalog, setCatalog] = useState(null);
  const [loadError, setLoadError] = useState('');
  const [attachment, setAttachment] = useState(null);
  const [count, setCount] = useState('1');

  useEffect(() => {
    const controller = new AbortController();
    api.getAttachmentCatalog(controller.signal).then(setCatalog).catch(error => {
      if (!controller.signal.aborted) setLoadError(error.message);
    });
    return () => controller.abort();
  }, [api]);

  const ownedCount = id => owned?.[id] ?? 0;
  const remaining = item => item.max_owned === null
    ? maxGrantCount
    : Math.max(0, Math.min(maxGrantCount, item.max_owned - ownedCount(item.id)));
  const limit = attachment ? remaining(attachment) : maxGrantCount;
  const value = Number(count);
  const valid = /^\d+$/.test(count) && value >= 1 && value <= limit;

  async function submit(event) {
    event.preventDefault();
    if (busy || !attachment || !valid) return;
    if (await onGrant({ kind: 'attachment', id: attachment.id, count: value })) {
      setAttachment(null);
      setCount('1');
    }
  }

  return (
    <Card component="form" onSubmit={submit}>
      <CardContent component={Stack} spacing={2}>
        <Typography variant="h6" component="h2">Grant a 3D attachment</Typography>
        {loadError && <Alert severity="error">{loadError}</Alert>}
        <Autocomplete options={catalog ?? []} value={attachment}
          onChange={(_, selection) => { setAttachment(selection); setCount('1'); }}
          getOptionKey={item => item.id} isOptionEqualToValue={(a, b) => a.id === b.id}
          getOptionLabel={item => item.name}
          getOptionDisabled={item => remaining(item) === 0}
          loading={catalog === null && !loadError} disabled={busy || !!loadError}
          noOptionsText={catalog?.length === 0 ? 'No grantable attachments in the server catalog' : 'No matching attachments'}
          renderOption={(props, item) => {
            const { key, ...attributes } = props;
            return <li key={key} {...attributes}>
              <Stack>
                <span>{item.name}</span>
                <Typography variant="caption" color="text.secondary">
                  {ownedCount(item.id)} owned
                </Typography>
              </Stack>
            </li>;
          }}
          renderInput={params => <TextField {...params} label="3D attachment" placeholder="Search by name" />} />
        <TextField label="Copies to add" value={count} onChange={event => setCount(event.target.value)}
          slotProps={{ htmlInput: { inputMode: 'numeric', maxLength: 3 } }}
          helperText={attachment && `${ownedCount(attachment.id)} owned · Up to ${limit} per grant`}
          error={!!attachment && !valid} required disabled={busy || !attachment} fullWidth />
        <Button type="submit" variant="contained" disabled={busy || !attachment || !valid} sx={{ alignSelf: 'start' }}>
          Grant attachment
        </Button>
      </CardContent>
    </Card>
  );
}
