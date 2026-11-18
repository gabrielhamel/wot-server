import type { ThemeOptions } from "@mui/material/styles";

import "@fontsource-variable/inter/wght.css";

export const serverTheme: ThemeOptions = {
  palette: {
    mode: "dark",
    primary: { main: "#e4bd7c", contrastText: "#22190e" },
    secondary: { main: "#dfe5ed" },
    background: { default: "#11161d", paper: "#1a222c" },
    text: { primary: "#eef1f5", secondary: "#adb8c7" },
    divider: "#333d4a",
  },
  shape: { borderRadius: 8 },
  typography: {
    fontFamily: '"Inter Variable", sans-serif',
    h5: { fontSize: "1.3rem", fontWeight: 650 },
    h6: { fontWeight: 650 },
    button: { fontWeight: 650, textTransform: "none" },
  },
  components: {
    MuiCard: {
      styleOverrides: { root: { backgroundImage: "none", border: "1px solid #333d4a" } },
    },
    MuiTableCell: {
      styleOverrides: {
        head: { fontWeight: 650 },
        root: { paddingTop: 13, paddingBottom: 13 },
      },
    },
  },
};
