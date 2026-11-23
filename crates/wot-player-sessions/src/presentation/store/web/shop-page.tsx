import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Box,
  Button,
  Container,
  CssBaseline,
  Skeleton,
  Tab,
  Tabs,
  Typography,
} from "@mui/material";
import { createTheme, ThemeProvider } from "@mui/material/styles";
import { serverTheme } from "../../../../../wot-platform/src/presentation/web/theme";
import type { StoreCategory, StoreTransport } from "../../../application/store-contracts";
import { garageStoreTransport } from "../../../infrastructure/store-api";
import { OfferCard } from "./offer-card";
import { useStoreCatalog } from "./use-store-catalog";
import { VehicleFilters } from "./vehicle-filters";

const theme = createTheme(serverTheme);

const sections: { title: string; categories: { id: StoreCategory; title: string }[] }[] = [
  {
    title: "WoT Premium Account",
    categories: [{ id: "premium_account", title: "WoT Premium Account" }],
  },
  {
    title: "Vehicles",
    categories: [
      { id: "premium_vehicles", title: "Premium vehicles" },
      { id: "collectors_vehicles", title: "Collectors’ vehicles" },
    ],
  },
  {
    title: "Exterior",
    categories: [
      { id: "styles2d", title: "2D styles" },
      { id: "styles3d", title: "3D styles" },
      { id: "crew_skins", title: "Crew skins" },
    ],
  },
  {
    title: "Service",
    categories: [
      { id: "equipment", title: "Standard equipment" },
      { id: "consumables", title: "Consumables" },
      { id: "crew_directives", title: "Directives for crew" },
    ],
  },
  {
    title: "Items for Bonds",
    categories: [
      { id: "bond_vehicles", title: "Vehicles" },
      { id: "improved_equipment", title: "Improved equipment" },
      { id: "equipment_directives", title: "Directives for equipment" },
      { id: "styles2d", title: "2D styles" },
      { id: "styles3d", title: "3D styles" },
    ],
  },
];

const catalogGrid = {
  display: "grid",
  gridTemplateColumns: {
    xs: "1fr",
    sm: "repeat(2, minmax(0, 1fr))",
    md: "repeat(3, minmax(0, 1fr))",
  },
  gap: { xs: 2, md: 2.5 },
};

function PremiumStatus({ expiresAt }: { expiresAt: number | null }) {
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (expiresAt === null || expiresAt * 1_000 <= now) return;
    const timer = window.setTimeout(
      () => setNow(Date.now()),
      Math.min(expiresAt * 1_000 - now, 60_000),
    );
    return () => window.clearTimeout(timer);
  }, [expiresAt, now]);
  const active = expiresAt !== null && expiresAt * 1_000 > now;
  return (
    <Typography color="text.secondary" sx={{ mb: 2.5 }}>
      {active
        ? `Active until ${new Intl.DateTimeFormat(undefined, { dateStyle: "long", timeStyle: "short" }).format(expiresAt * 1_000)}. Purchases extend your remaining time.`
        : "Choose how long to activate your WoT Premium Account."}
    </Typography>
  );
}

function Store({
  transport,
  embedded = false,
  notifyNativeClient = false,
  onUnauthorized,
  onPurchased,
}: {
  transport: StoreTransport;
  embedded?: boolean;
  notifyNativeClient?: boolean;
  onUnauthorized?: () => void;
  onPurchased?: () => void;
}) {
  const { catalog, loading, buyingOffer, purchasedOffer, error, reload, buy } = useStoreCatalog(
    transport,
    notifyNativeClient,
    onUnauthorized,
    onPurchased,
  );
  const [section, setSection] = useState(sections[0]);
  const [category, setCategory] = useState(sections[0].categories[0]);
  const [nation, setNation] = useState("all");
  const [tier, setTier] = useState("all");
  const sectionIndex = sections.indexOf(section);
  const hasCategories = section.categories.length > 1;
  const offers = catalog?.offers.filter((offer) => offer.category === category.id) ?? [];
  const vehicles =
    category.id === "premium_vehicles" ||
    category.id === "collectors_vehicles" ||
    category.id === "bond_vehicles";
  const visible = offers.filter(
    (offer) =>
      (nation === "all" || offer.nation === nation) &&
      (tier === "all" || String(offer.tier) === tier),
  );

  function selectCategory(next: typeof category) {
    setCategory(next);
    setNation("all");
    setTier("all");
  }

  return (
    <Container
      component={embedded ? "section" : "main"}
      maxWidth="lg"
      disableGutters={embedded}
      sx={embedded ? undefined : { py: { xs: 2.5, md: 4 } }}
    >
      <Tabs
        value={section.title}
        aria-label="Store sections"
        variant="scrollable"
        scrollButtons="auto"
        onChange={(_, title: string) => {
          const next = sections.find((candidate) => candidate.title === title);
          if (next) {
            setSection(next);
            selectCategory(next.categories[0]);
          }
        }}
        sx={{ borderBottom: 1, borderColor: "divider", mb: 2.5 }}
      >
        {sections.map(({ title }, index) => (
          <Tab
            key={title}
            id={`store-section-${index}`}
            aria-controls="store-section-panel"
            label={title}
            value={title}
          />
        ))}
      </Tabs>
      {embedded && purchasedOffer && (
        <Alert role="status" severity="success" sx={{ mb: 2.5 }}>
          Purchased {purchasedOffer.name}.
        </Alert>
      )}
      <Box
        id="store-section-panel"
        role="tabpanel"
        aria-labelledby={`store-section-${sectionIndex}`}
      >
        {hasCategories && (
          <Tabs
            value={category.id}
            aria-label={`${section.title} categories`}
            variant="scrollable"
            onChange={(_, id: StoreCategory) => {
              const next = section.categories.find((candidate) => candidate.id === id);
              if (next) selectCategory(next);
            }}
            sx={{ mb: 2.5 }}
          >
            {section.categories.map(({ id, title }) => (
              <Tab
                key={id}
                id={`store-category-${id}`}
                aria-controls="store-category-panel"
                label={title}
                value={id}
              />
            ))}
          </Tabs>
        )}
        <Box
          id="store-category-panel"
          role={hasCategories ? "tabpanel" : undefined}
          aria-labelledby={hasCategories ? `store-category-${category.id}` : undefined}
        >
          {error !== null && (
            <Alert
              severity="error"
              sx={{ mb: 2.5 }}
              action={
                <Button color="inherit" size="small" onClick={reload}>
                  Reload offers
                </Button>
              }
            >
              {error}
            </Alert>
          )}
          {loading && (
            <Box role="status" aria-label="Loading offers" sx={catalogGrid}>
              {[0, 1, 2].map((item) => (
                <Skeleton key={item} variant="rounded" height={152} />
              ))}
            </Box>
          )}
          {!loading && catalog && (
            <Box component="section" aria-labelledby="store-category-title">
              <Typography
                id="store-category-title"
                component={embedded ? "h2" : "h1"}
                variant="h5"
                sx={{ mb: 1.5 }}
              >
                {category.title}
              </Typography>
              {category.id === "premium_account" && (
                <PremiumStatus expiresAt={catalog.premium_expires_at} />
              )}
              {vehicles && offers.length > 0 && (
                <VehicleFilters
                  offers={offers}
                  nation={nation}
                  tier={tier}
                  setNation={setNation}
                  setTier={setTier}
                />
              )}
              {visible.length === 0 ? (
                <Alert severity="info">No offers match this selection.</Alert>
              ) : (
                <Box sx={catalogGrid}>
                  {visible.map((offer) => (
                    <OfferCard
                      key={`${offer.item}:${offer.price.currency}`}
                      offer={offer}
                      buying={buyingOffer === offer}
                      disabled={buyingOffer !== null || error !== null}
                      onBuy={buy}
                    />
                  ))}
                </Box>
              )}
            </Box>
          )}
        </Box>
      </Box>
    </Container>
  );
}

export function EmbeddedStore({
  transport,
  onUnauthorized,
  onPurchased,
}: {
  transport: StoreTransport;
  onUnauthorized: () => void;
  onPurchased: () => void;
}) {
  return (
    <Store
      transport={transport}
      embedded
      onUnauthorized={onUnauthorized}
      onPurchased={onPurchased}
    />
  );
}

export default function Shop() {
  const token = /^#([A-Za-z0-9_-]{43})(?=$|[/?&])/.exec(window.location.hash)?.[1];
  const transport = useMemo(() => (token ? garageStoreTransport(token) : null), [token]);
  return (
    <ThemeProvider theme={theme}>
      <CssBaseline />
      {transport ? (
        <Store transport={transport} notifyNativeClient />
      ) : (
        <Container component="main" maxWidth="lg" sx={{ py: { xs: 2.5, md: 4 } }}>
          <Alert severity="info">Open the Store from a connected garage.</Alert>
        </Container>
      )}
    </ThemeProvider>
  );
}
