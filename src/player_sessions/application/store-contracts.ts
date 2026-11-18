export interface StoreOffer {
  item: number;
  name: string;
  credits: number;
  in_depot: number;
}

export interface StoreCatalog {
  revision: string;
  credits: number;
  offers: StoreOffer[];
}

export interface StoreOrder {
  item: number;
  revision: string;
  credits: number;
}
