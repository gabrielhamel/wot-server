import ShieldOutlined from "@mui/icons-material/ShieldOutlined";
import { Typography } from "@mui/material";
import { AppBar, Layout, TitlePortal } from "react-admin";
import type { AppBarProps, LayoutProps } from "react-admin";

function ServerBar(props: AppBarProps) {
  return (
    <AppBar
      {...props}
      color="transparent"
      elevation={0}
      sx={{ bgcolor: "background.paper", borderBottom: 1, borderColor: "divider" }}
    >
      <ShieldOutlined sx={{ mr: 1, color: "primary.main", display: { xs: "none", sm: "block" } }} />
      <Typography variant="subtitle2" sx={{ mr: 3, display: { xs: "none", md: "block" } }}>
        World of Tanks Server
      </Typography>
      <TitlePortal />
    </AppBar>
  );
}

export function ServerLayout(props: LayoutProps) {
  return (
    <Layout
      {...props}
      appBar={ServerBar}
      appBarAlwaysOn
      sx={{ "& .RaLayout-content": { px: { xs: 1, sm: 3 }, pt: { xs: 1, sm: 2 } } }}
    />
  );
}
