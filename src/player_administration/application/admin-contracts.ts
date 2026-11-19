export type PlayerRole = "player" | "admin";

export interface PlayerRecord {
  id: string;
  username: string;
  display_name: string;
  role: PlayerRole;
}

export type NewPlayer = Omit<PlayerRecord, "id" | "role"> & { password: string };

export const currencies = [
  ["credits", "Credits"],
  ["gold", "Gold"],
  ["bonds", "Bonds"],
  ["free_xp", "Free XP"],
] as const;

export type Currency = (typeof currencies)[number][0];
export type OwnedCosmetics = Record<number, number>;

export interface Vehicle {
  key: string;
  name: string;
  tier: number | null;
}

export interface CatalogVehicle extends Vehicle {
  id: number;
}

export interface GarageVehicle extends Vehicle {
  inventory_id: number;
  stat_trackers: OwnedCosmetics;
}

export type PlayerGarage = Record<Currency, string> & {
  display_name: string;
  slots: number;
  vehicles: Vehicle[];
};

export type Garage = Omit<PlayerGarage, "vehicles"> & {
  vehicles: GarageVehicle[];
  attachments: OwnedCosmetics;
};

export interface Cosmetic {
  id: number;
  name: string;
  max_owned: number | null;
}

export type Grant =
  | { kind: "currency"; currency: Currency; amount: number }
  | { kind: "vehicle"; vehicle: number }
  | { kind: "attachment"; id: number; count: number }
  | { kind: "stat_tracker"; vehicle: number; id: number; count: number };

export interface GrantResult {
  garage: Garage;
  delivery: "queued_for_garage" | "on_next_sync";
}

export type Registration = {
  email: string;
  username: string;
  password: string;
  confirm_password: string;
};

export type AdminSession =
  | { status: "setup_required" }
  | { status: "signed_out"; registration_open: boolean }
  | { status: "authenticated"; player: PlayerRecord; store_enabled: boolean };
