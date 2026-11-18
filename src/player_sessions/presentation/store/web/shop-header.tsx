import AccountBalanceWalletOutlined from "@mui/icons-material/AccountBalanceWalletOutlined";
import { Box, Paper, Stack, Typography } from "@mui/material";

const number = new Intl.NumberFormat("en");

export function ShopHeader({ credits }: { credits?: number }) {
  return (
    <Paper
      component="header"
      variant="outlined"
      sx={{
        p: { xs: 3, md: 4 },
        mb: 3,
        backgroundImage: "linear-gradient(120deg, #242f3d, #1a222c 65%)",
      }}
    >
      <Stack
        direction={{ xs: "column", sm: "row" }}
        spacing={3}
        justifyContent="space-between"
        alignItems={{ sm: "center" }}
      >
        <Box>
          <Typography
            component="h1"
            variant="h3"
            sx={{ fontWeight: 750, fontSize: { xs: "2rem", md: "2.75rem" } }}
          >
            Equipment
          </Typography>
          <Typography color="text.secondary" sx={{ mt: 0.75 }}>
            Purchases go to your Depot.
          </Typography>
        </Box>
        {credits != null && (
          <Stack
            direction="row"
            spacing={1.5}
            alignItems="center"
            sx={{
              alignSelf: { xs: "flex-start", sm: "auto" },
              bgcolor: "background.default",
              border: 1,
              borderColor: "divider",
              borderRadius: 1,
              px: 2,
              py: 1.25,
            }}
          >
            <AccountBalanceWalletOutlined color="primary" />
            <Box>
              <Typography variant="caption" color="text.secondary">
                Available credits
              </Typography>
              <Typography
                variant="h6"
                sx={{ lineHeight: 1.25, fontVariantNumeric: "tabular-nums" }}
              >
                {number.format(credits)}
              </Typography>
            </Box>
          </Stack>
        )}
      </Stack>
    </Paper>
  );
}
