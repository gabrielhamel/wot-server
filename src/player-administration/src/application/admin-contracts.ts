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
  ["equipment_components", "Experimental Equipment components"],
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
  styles_2d: OwnedCosmetics;
  styles_3d: OwnedCosmetics;
  crew_skins: OwnedCosmetics;
  special_recruits: OwnedCosmetics;
  depot: Record<number, number>;
};

export interface DepotItem {
  id: number;
  name: string;
}

export interface SpecialRecruit {
  id: number;
  name: string;
  countries: DepotItem[];
}

export type DepotGrantKind =
  | "crew_book"
  | "experimental_equipment"
  | "bounty_equipment"
  | "improved_equipment";

export interface Cosmetic {
  id: number;
  name: string;
  max_owned: number | null;
}

export type Grant =
  | { kind: "currency"; currency: Currency; amount: number }
  | { kind: "vehicle"; vehicle: number }
  | { kind: DepotGrantKind; id: number; count: number }
  | { kind: "attachment" | "style2d" | "style3d" | "crew_skin"; id: number; count: number }
  | { kind: "special_recruit"; id: number }
  | { kind: "stat_tracker"; vehicle: number; id: number; count: number };

export interface GrantResult {
  garage: Garage;
  delivery: "queued_for_garage" | "on_next_sync";
}

export type PlayerCredentials = {
  email: string;
  username: string;
  password: string;
};

export type Registration = PlayerCredentials & { confirm_password: string };

export type AdminSession =
  | { status: "setup_required" }
  | { status: "signed_out"; registration_open: boolean }
  | { status: "authenticated"; player: PlayerRecord; store_enabled: boolean };
