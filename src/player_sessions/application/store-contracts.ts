export type StoreCategory =
  | "premium_account"
  | "premium_vehicles"
  | "collectors_vehicles"
  | "crew_skins"
  | "styles2d"
  | "styles3d"
  | "equipment"
  | "consumables"
  | "crew_directives"
  | "bond_vehicles"
  | "improved_equipment"
  | "equipment_directives";

export interface StorePrice {
  currency: "credits" | "gold" | "bonds";
  amount: number;
}

export interface StoreOffer {
  item: number;
  name: string;
  category: StoreCategory;
  price: StorePrice;
  availability:
    | "available"
    | "insufficient_funds"
    | "research_required"
    | "in_garage"
    | "garage_full"
    | "ownership_limit";
  in_depot: number;
  nation?: string;
  tier?: number;
}

export interface StoreCatalog {
  revision: string;
  premium_expires_at: number | null;
  offers: StoreOffer[];
}

export interface StoreOrder {
  item: number;
  revision: string;
  price: StorePrice;
}

export type StoreAccess = { kind: "garage"; token: string } | { kind: "dashboard" };
