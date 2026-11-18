use super::{
    BTreeMap, BTreeSet, CosmeticCatalog, CosmeticDefinition, CosmeticKind, CosmeticPlacement,
    CosmeticRule, OutfitSeason, Result, STYLE_TYPE, STYLES, VehicleFilter, cosmetic_definitions,
    cosmetic_descriptor, item_defs,
};
use crate::accounts::infrastructure::client::item_prices::ItemPrices;
use crate::accounts::infrastructure::client::store::{
    StoreCategory, StoreOffer, StorePrice, listing,
};
use wot_resources::ResourceBundle;

// Prices and labels for regular bond offers in PR #45. The installed client supplies
// the style IDs, 2D/3D tags, and compatible vehicles.
const OFFERS: &[(u16, &str, i64)] = &[
    (351, "Fox Fur (CS-52 LIS)", 350),
    (291, "Hot TOG (TOG II*)", 450),
    (195, "Moon Viewing", 450),
    (237, "Boxing Kangaroo", 450),
    (279, "Songkran", 450),
    (280, "Sakura Storm", 450),
    (433, "Rat Rod", 450),
    (46, "Ice Shelf", 450),
    (489, "Into Space!", 450),
    (685, "such camo, much wow", 450),
    (2223, "Tree Frog", 450),
    (267, "Conquer the Stars", 450),
    (269, "Semper Feles!", 450),
    (380, "Hou Yi’s Honor", 450),
    (384, "Dazzle", 450),
    (437, "Overlord", 450),
    (47, "Bargain", 450),
    (500, "Chinese Ceramics", 450),
    (525, "Red Desert", 450),
    (526, "Gunsmith", 450),
    (574, "Battlefield", 450),
    (813, "Ace", 450),
    (827, "Mid-Autumn Festival", 450),
    (828, "Moon Rabbit", 450),
    (225, "Snow Leopard (SU-130PM)", 350),
    (484, "Special Guest (122 TM)", 2_000),
    (111, "Montu (IS-3)", 2_000),
    (12, "Odin’s Wolves (Lansen C)", 2_000),
    (87, "Galahad (FV4005 Stage II)", 3_000),
    (444, "Kaiju (Type 5 H)", 3_000),
    (228, "Granite (IS-7)", 3_000),
    (229, "Marengo (B-C 25 t)", 3_000),
    (440, "40:1 (60TP)", 3_000),
    (649, "Swordfish (M-V-Y)", 3_000),
    (432, "The Last Dragon (WZ-111 model 5A)", 3_000),
    (429, "Landkreuzer (Maus)", 3_000),
    (334, "Saiga (ST-II)", 3_000),
    (442, "Fair Trade (Object 268 Version 4)", 3_000),
    (100, "Klarer Himmel (E 100)", 3_000),
    (441, "Winter Lotus (STB-1)", 3_000),
    (101, "Major Ram (T110E4)", 3_000),
    (97, "Vipère Noire (AMX 13 105)", 3_000),
    (98, "Sekhmet (Obj. 140)", 3_000),
    (99, "Kelevra (Obj. 430U)", 3_000),
    (112, "Panzerschiff (Jg.Pz. E 100)", 3_000),
    (578, "Hurricane (Vz. 55)", 3_000),
    (302, "Salamander (Sheridan)", 3_000),
];

pub(super) fn load(
    catalog: &mut CosmeticCatalog,
    fs: &ResourceBundle,
    prices: &mut ItemPrices,
) -> Result<()> {
    if !fs.has_file(STYLES) {
        return Ok(());
    }
    let offers: BTreeMap<_, _> = OFFERS
        .iter()
        .map(|&(id, name, price)| (id, (name, price)))
        .collect();
    let styles = item_defs::read_xml(fs, STYLES)?;
    for (id, style, group) in cosmetic_definitions(&styles, "style") {
        let id = id?;
        let Some(&(name, price)) = offers.get(&id) else {
            continue;
        };
        let tags: BTreeSet<_> = style
            .get_child_string("tags")
            .or_else(|| group.get_child_string("tags"))
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        if [
            "hiddenInUI",
            "lockedOnVehicle",
            "styleProgression",
            "styleSerialNumber",
        ]
        .iter()
        .any(|tag| tags.contains(tag))
        {
            continue;
        }
        let category = if tags.contains("c11n3D") {
            StoreCategory::Styles3d
        } else if tags.contains("c11n2D") {
            StoreCategory::Styles2d
        } else {
            continue;
        };
        let mut include = Vec::new();
        let mut exclude = Vec::new();
        if let Some(filter) = style
            .get_child_element("vehicleFilter")
            .or_else(|| group.get_child_element("vehicleFilter"))
        {
            for (kind, node) in filter.iter_child_elements() {
                match kind {
                    "include" => include.push(VehicleFilter::read(node)?),
                    "exclude" => exclude.push(VehicleFilter::read(node)?),
                    _ => anyhow::bail!("Unknown style vehicle filter {kind}"),
                }
            }
        }
        let max_owned = style
            .get_child_integer("maxNumber")
            .or_else(|| group.get_child_integer("maxNumber"))
            .map(u32::try_from)
            .transpose()?;
        catalog.definitions.insert(
            (CosmeticKind::Style, id),
            CosmeticDefinition {
                rule: CosmeticRule {
                    price: None,
                    seasons: BTreeSet::from([OutfitSeason::Common]),
                    placement: CosmeticPlacement::Style,
                    max_owned,
                },
                available_for_custom_outfits: true,
                include,
                exclude,
            },
        );
        let descriptor = cosmetic_descriptor(STYLE_TYPE, id);
        // The client enumerates style inventory through shop-price membership.
        // An empty price entry exposes the item without enabling native checkout.
        prices.catalog_only.push(descriptor);
        let item = i32::try_from(descriptor.to_int())?;
        catalog.store_styles.push(listing(
            item,
            name.to_owned(),
            category,
            StorePrice::Bonds(price),
        ));
    }
    Ok(())
}

impl CosmeticCatalog {
    pub(in crate::accounts::infrastructure::client) fn store_styles(&self) -> &[StoreOffer] {
        &self.store_styles
    }

    pub(in crate::accounts::infrastructure::client) fn store_style_limit(
        &self,
        id: u16,
    ) -> Option<u32> {
        self.definitions
            .get(&(CosmeticKind::Style, id))
            .and_then(|definition| definition.rule.max_owned)
    }
}
