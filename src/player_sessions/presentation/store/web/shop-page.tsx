import { Alert, Box, Button, Container, CssBaseline, Skeleton } from "@mui/material";
import { createTheme, ThemeProvider } from "@mui/material/styles";
import { serverTheme } from "../../../../platform/presentation/web/theme";
import { ShopHeader } from "./shop-header";
import { OfferCard } from "./offer-card";
import { useEquipmentCatalog } from "./use-equipment-catalog";

const theme = createTheme(serverTheme);

const catalogGrid = {
  display: "grid",
  gridTemplateColumns: {
    xs: "1fr",
    sm: "repeat(2, minmax(0, 1fr))",
    md: "repeat(3, minmax(0, 1fr))",
  },
  gap: { xs: 2, md: 2.5 },
};

function EquipmentCatalog({ token }: { token?: string }) {
  const { catalog, loading, buyingItem, feedback, reload, buy } = useEquipmentCatalog(token);
  const needsReload = feedback?.severity === "error";
  return (
    <Container component="main" maxWidth="lg" sx={{ py: { xs: 3, md: 5 } }}>
      <ShopHeader credits={catalog?.credits} />
      {!token && <Alert severity="info">Open the Store from a connected garage.</Alert>}
      {feedback && (
        <Alert
          severity={feedback.severity}
          sx={{ mb: 2.5 }}
          action={
            needsReload && (
              <Button color="inherit" size="small" onClick={reload}>
                Reload offers
              </Button>
            )
          }
        >
          {feedback.message}
        </Alert>
      )}
      {loading && (
        <Box role="status" aria-label="Loading offers" sx={catalogGrid}>
          {[0, 1, 2].map((item) => (
            <Skeleton key={item} variant="rounded" height={152} />
          ))}
        </Box>
      )}
      {!loading && catalog?.offers.length === 0 && (
        <Alert severity="info">No equipment offers are available.</Alert>
      )}
      {!loading && catalog && catalog.offers.length > 0 && (
        <Box component="section" aria-label="Equipment offers" sx={catalogGrid}>
          {catalog.offers.map((offer) => (
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
  const token = /^#([A-Za-z0-9_-]{43})(?=$|[/?&])/.exec(window.location.hash)?.[1];
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      <EquipmentCatalog token={token} />
    </ThemeProvider>
  );
}
