import { MenuItem, Stack, TextField } from "@mui/material";
import type { StoreOffer } from "../../../application/store-contracts";

const nations: Record<string, string> = {
  china: "China",
  czech: "Czechoslovakia",
  france: "France",
  germany: "Germany",
  italy: "Italy",
  japan: "Japan",
  poland: "Poland",
  sweden: "Sweden",
  uk: "U.K.",
  usa: "U.S.A.",
  ussr: "U.S.S.R.",
};

export function nationName(nation: string) {
  return nations[nation] ?? "Other nation";
}

export function tierName(tier: number) {
  return (
    ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI"][tier - 1] ?? String(tier)
  );
}

export function VehicleFilters({
  offers,
  nation,
  tier,
  setNation,
  setTier,
}: {
  offers: StoreOffer[];
  nation: string;
  tier: string;
  setNation: (nation: string) => void;
  setTier: (tier: string) => void;
}) {
  const availableNations = Object.keys(nations).filter((nation) =>
    offers.some((offer) => offer.nation === nation),
  );
  const tiers = [
    ...new Set(offers.flatMap((offer) => (offer.tier === undefined ? [] : [offer.tier]))),
  ].sort((a, b) => a - b);
  return (
    <Stack direction={{ xs: "column", sm: "row" }} gap={1.5} sx={{ mb: 2.5 }}>
      <TextField
        select
        label="Nation"
        size="small"
        value={nation}
        onChange={(event) => setNation(event.target.value)}
        sx={{ minWidth: 180 }}
      >
        <MenuItem value="all">All nations</MenuItem>
        {availableNations.map((nation) => (
          <MenuItem key={nation} value={nation}>
            {nationName(nation)}
          </MenuItem>
        ))}
      </TextField>
      <TextField
        select
        label="Tier"
        size="small"
        value={tier}
        onChange={(event) => setTier(event.target.value)}
        sx={{ minWidth: 140 }}
      >
        <MenuItem value="all">All tiers</MenuItem>
        {tiers.map((tier) => (
          <MenuItem key={tier} value={String(tier)}>
            {tierName(tier)}
          </MenuItem>
        ))}
      </TextField>
    </Stack>
  );
}
