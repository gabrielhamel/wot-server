import { Button, Card, CardActions, CardContent, Stack, Typography } from "@mui/material";
import type { StoreOffer } from "../../../application/store-contracts";
import { nationName, tierName } from "./vehicle-filters";

const number = new Intl.NumberFormat("en");
const availabilityText = {
  available: "",
  insufficient_funds: "Insufficient funds",
  research_required: "Research a Tech Tree vehicle of this tier or higher in this nation",
  in_garage: "In Garage",
  garage_full: "Garage full",
  ownership_limit: "Ownership limit reached",
};

interface OfferCardProps {
  offer: StoreOffer;
  disabled: boolean;
  buying: boolean;
  onBuy: (offer: StoreOffer) => void;
}

export function OfferCard({ offer, disabled, buying, onBuy }: OfferCardProps) {
  const price = `${number.format(offer.price.amount)} ${offer.price.currency}`;
  const status = availabilityText[offer.availability];
  return (
    <Card component="article" variant="outlined" sx={{ display: "flex", flexDirection: "column" }}>
      <CardContent sx={{ p: 2.5, flexGrow: 1 }}>
        <Stack gap={0.75}>
          {offer.nation && offer.tier !== undefined && (
            <Typography variant="overline" color="text.secondary">
              {nationName(offer.nation)} · Tier {tierName(offer.tier)}
            </Typography>
          )}
          <Typography component="h2" variant="h6" sx={{ lineHeight: 1.35 }}>
            {offer.name}
          </Typography>
          {offer.tier === undefined && offer.category !== "premium_account" && (
            <Typography variant="body2" color="text.secondary">
              {number.format(offer.in_depot)} in Depot
            </Typography>
          )}
          {status && (
            <Typography variant="body2" color="text.secondary">
              {status}
            </Typography>
          )}
        </Stack>
      </CardContent>
      {offer.availability !== "in_garage" && (
        <CardActions sx={{ px: 2.5, pb: 2.5, pt: 0 }}>
          <Button
            fullWidth
            variant="contained"
            disabled={disabled || offer.availability !== "available"}
            loading={buying}
            onClick={() => onBuy(offer)}
            aria-label={buying ? `Buying ${offer.name}` : `Buy ${offer.name} for ${price}`}
          >
            Buy for {price}
          </Button>
        </CardActions>
      )}
    </Card>
  );
}
