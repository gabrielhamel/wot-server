import { useCallback, useEffect, useState } from 'react';
import AccountBalanceWalletOutlined from '@mui/icons-material/AccountBalanceWalletOutlined';
import Inventory2Outlined from '@mui/icons-material/Inventory2Outlined';
import ShoppingCartOutlined from '@mui/icons-material/ShoppingCartOutlined';
import {
  Alert, Box, Button, Card, CardActions, CardContent, Chip, Container, CssBaseline,
  Paper, Skeleton, Stack, Typography,
} from '@mui/material';
import { createTheme, ThemeProvider } from '@mui/material/styles';
import { serverTheme } from './theme';

const token = /^#([A-Za-z0-9_-]{43})(?=$|[/?&])/.exec(window.location.hash)?.[1];
const number = new Intl.NumberFormat('en');
const theme = createTheme(serverTheme);

const catalogGrid = {
  display: 'grid',
  gridTemplateColumns: { xs: '1fr', sm: 'repeat(2, minmax(0, 1fr))', md: 'repeat(3, minmax(0, 1fr))' },
  gap: { xs: 2, md: 2.5 },
};

async function request(endpoint, order, signal) {
  const headers = { Authorization: `Bearer ${token}` };
  if (order) headers['Content-Type'] = 'application/json';
  const response = await fetch(`/api/shop/${endpoint}`, {
    method: order ? 'POST' : 'GET',
    headers,
    body: order ? JSON.stringify(order) : undefined,
    cache: 'no-store',
    redirect: 'error',
    signal,
  });
  if (!response.ok) {
    throw new Error(await response.text() || 'The Store is temporarily unavailable.');
  }
  return response.json();
}

function ShopHeader({ credits }) {
  return (
    <Paper
      component="header"
      variant="outlined"
      sx={{
        p: { xs: 3, md: 4 },
        mb: 3,
        backgroundImage: 'linear-gradient(120deg, #242f3d, #1a222c 65%)',
      }}
    >
      <Stack direction={{ xs: 'column', sm: 'row' }} spacing={3} justifyContent="space-between"
        alignItems={{ sm: 'center' }}>
        <Box>
          <Typography component="h1" variant="h3" sx={{ fontWeight: 750, fontSize: { xs: '2rem', md: '2.75rem' } }}>
            Equipment
          </Typography>
          <Typography color="text.secondary" sx={{ mt: 0.75 }}>
            Purchases go to your Depot.
          </Typography>
        </Box>
        {credits != null && (
          <Stack direction="row" spacing={1.5} alignItems="center" sx={{
            alignSelf: { xs: 'flex-start', sm: 'auto' },
            bgcolor: 'background.default', border: 1, borderColor: 'divider', borderRadius: 1,
            px: 2, py: 1.25,
          }}>
            <AccountBalanceWalletOutlined color="primary" />
            <Box>
              <Typography variant="caption" color="text.secondary">Available credits</Typography>
              <Typography variant="h6" sx={{ lineHeight: 1.25, fontVariantNumeric: 'tabular-nums' }}>
                {number.format(credits)}
              </Typography>
            </Box>
          </Stack>
        )}
      </Stack>
    </Paper>
  );
}

function OfferCard({ offer, credits, disabled, buying, onBuy }) {
  const price = `${number.format(offer.credits)} credits`;
  return (
    <Card component="article" sx={{ display: 'flex', flexDirection: 'column' }}>
      <CardContent sx={{ p: 2.5, flexGrow: 1 }}>
        <Chip
          variant="outlined"
          size="small"
          icon={<Inventory2Outlined />}
          label={`${number.format(offer.in_depot)} in Depot`}
        />
        <Typography component="h2" variant="h6" sx={{ mt: 1.25, lineHeight: 1.35 }}>
          {offer.name}
        </Typography>
      </CardContent>
      <CardActions sx={{ px: 2.5, pb: 2.5, pt: 0 }}>
        <Button
          fullWidth
          variant="contained"
          startIcon={<ShoppingCartOutlined />}
          disabled={disabled || credits < offer.credits}
          loading={buying}
          onClick={() => onBuy(offer)}
          aria-label={`Buy ${offer.name} for ${price}`}
        >
          Buy for {price}
        </Button>
      </CardActions>
    </Card>
  );
}

function EquipmentCatalog() {
  const [catalog, setCatalog] = useState(null);
  const [loading, setLoading] = useState(Boolean(token));
  const [buyingItem, setBuyingItem] = useState(null);
  const [feedback, setFeedback] = useState(null);

  const load = useCallback(async (signal) => {
    setLoading(true);
    setFeedback(null);
    try {
      setCatalog(await request('catalog', undefined, signal));
    } catch (error) {
      if (!signal?.aborted) setFeedback({ severity: 'error', message: error.message });
    } finally {
      if (!signal?.aborted) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!token) return;
    const controller = new AbortController();
    load(controller.signal);
    return () => controller.abort();
  }, [load]);

  async function buy(offer) {
    setBuyingItem(offer.item);
    setFeedback(null);
    try {
      const result = await request('orders', {
        item: offer.item,
        revision: catalog.revision,
        credits: offer.credits,
      });
      setCatalog(result);
      setFeedback({ severity: 'success', message: `${offer.name} added to your Depot.` });
    } catch (error) {
      setFeedback({ severity: 'error', message: error.message });
    } finally {
      setBuyingItem(null);
    }
  }

  const needsReload = feedback?.severity === 'error';
  return (
    <Container component="main" maxWidth="lg" sx={{ py: { xs: 3, md: 5 } }}>
      <ShopHeader credits={catalog?.credits} />
      {!token && <Alert severity="info">Open the Store from a connected garage.</Alert>}
      {feedback && (
        <Alert severity={feedback.severity} sx={{ mb: 2.5 }} action={needsReload && (
          <Button color="inherit" size="small" onClick={() => load()}>Reload offers</Button>
        )}>
          {feedback.message}
        </Alert>
      )}
      {loading && (
        <Box role="status" aria-label="Loading offers" sx={catalogGrid}>
          {[0, 1, 2].map(item => <Skeleton key={item} variant="rounded" height={152} />)}
        </Box>
      )}
      {!loading && catalog?.offers.length === 0 && <Alert severity="info">No equipment offers are available.</Alert>}
      {!loading && Boolean(catalog?.offers.length) && (
        <Box component="section" aria-label="Equipment offers" sx={catalogGrid}>
          {catalog.offers.map(offer => (
            <OfferCard
              key={offer.item}
              offer={offer}
              credits={catalog.credits}
              disabled={buyingItem !== null || needsReload}
              buying={buyingItem === offer.item}
              onBuy={buy}
            />
          ))}
        </Box>
      )}
    </Container>
  );
}

export default function Shop() {
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <EquipmentCatalog />
    </ThemeProvider>
  );
}
