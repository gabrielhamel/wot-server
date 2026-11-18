import {
  Box,
  Card,
  CardContent,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  Typography,
} from "@mui/material";
import { currencies } from "../../../application/admin-contracts";
import type { Garage, Vehicle } from "../../../application/admin-contracts";

export function vehicleName(vehicle: Vehicle) {
  return vehicle.name && vehicle.name !== vehicle.key ? vehicle.name : "Unknown tank";
}

export function GarageBalances({ garage }: { garage: Garage }) {
  return (
    <Card>
      <CardContent>
        <Typography variant="h6" component="h2" gutterBottom>
          Current balances
        </Typography>
        <Box
          component="dl"
          sx={{
            display: "grid",
            gridTemplateColumns: { xs: "repeat(2, 1fr)", sm: "repeat(4, 1fr)" },
            gap: 2,
            m: 0,
          }}
        >
          {currencies.map(([key, label]) => (
            <Box key={key}>
              <Typography component="dt" variant="body2" color="text.secondary">
                {label}
              </Typography>
              <Typography component="dd" variant="h6" sx={{ m: 0 }}>
                {BigInt(garage[key]).toLocaleString()}
              </Typography>
            </Box>
          ))}
        </Box>
      </CardContent>
    </Card>
  );
}

export function OwnedTanks({ garage }: { garage: Garage }) {
  return (
    <Card>
      <CardContent>
        <Typography variant="h6" component="h2" gutterBottom>
          Owned tanks
        </Typography>
        <Typography variant="body2" color="text.secondary">
          {garage.vehicles.length} of {garage.slots} garage slots occupied
        </Typography>
      </CardContent>
      {garage.vehicles.length > 0 && (
        <Table size="small">
          <TableHead>
            <TableRow>
              <TableCell>Tank</TableCell>
              <TableCell align="right">Tier</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>
            {garage.vehicles.map((tank) => (
              <TableRow key={tank.key}>
                <TableCell component="th" scope="row">
                  {vehicleName(tank)}
                </TableCell>
                <TableCell align="right">{tank.tier ?? "—"}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      )}
    </Card>
  );
}
