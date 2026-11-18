import Inventory2Outlined from "@mui/icons-material/Inventory2Outlined";
import ShoppingCartOutlined from "@mui/icons-material/ShoppingCartOutlined";
import { Button, Card, CardActions, CardContent, Chip, Typography } from "@mui/material";
import type { StoreOffer } from "../../../application/store-contracts";

const number = new Intl.NumberFormat("en");

interface OfferCardProps {
  offer: StoreOffer;
  credits: number;
  disabled: boolean;
  buying: boolean;
  onBuy: (offer: StoreOffer) => void;
}

export function OfferCard({ offer, credits, disabled, buying, onBuy }: OfferCardProps) {
  const price = `${number.format(offer.credits)} credits`;
  return (
    <Card component="article" sx={{ display: "flex", flexDirection: "column" }}>
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
