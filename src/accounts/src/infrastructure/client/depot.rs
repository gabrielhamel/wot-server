use super::{
    GarageCatalog,
    items::{IntCompactDescr, item_type},
    shop::{AMMO_EXCHANGE_RATE, CRYSTAL_EXCHANGE_RATE, GOLD_EXCHANGE_RATE, SELL_PRICE_DIVISOR},
};
use crate::{
    AccountSnapshot, DepotProceeds, DepotSale, DepotSaleRequest, DepotStock, MAX_DEPOT_SALE_LINES,
    ProgressionError,
};

impl GarageCatalog {
    pub(super) fn sell_depot_items(
        &self,
        snapshot: &mut AccountSnapshot,
        requests: Vec<DepotSaleRequest>,
    ) -> Result<(), ProgressionError> {
        if !(1..=MAX_DEPOT_SALE_LINES).contains(&requests.len()) {
            return Err(ProgressionError::InvalidRequest);
        }
        let sales = requests
            .into_iter()
            .map(|request| self.price_depot_sale(request))
            .collect::<Result<Vec<_>, _>>()?;
        snapshot.sell_depot_stock(sales)
    }

    pub(super) fn price_depot_sale(
        &self,
        request: DepotSaleRequest,
    ) -> Result<DepotSale, ProgressionError> {
        let item = i64::from(request.item);
        let cd = IntCompactDescr::from_int(item);
        if item <= 0 || cd.to_int() != item || request.item_type != cd.item_type {
            return Err(ProgressionError::InvalidRequest);
        }
        let stock = match cd.item_type {
            item_type::SHELL => DepotStock::Shells,
            item_type::CHASSIS..=item_type::RADIO
            | item_type::OPTIONAL_DEVICE
            | item_type::EQUIPMENT
            | item_type::CREW_BOOK => DepotStock::Fittings,
            _ => return Err(ProgressionError::InvalidRequest),
        };
        let proceeds = match self.equipment.disassembly_value(request.item) {
            Some(value) => DepotProceeds::Components(value),
            None => DepotProceeds::Credits(self.sale_unit_credits(cd)?),
        };
        Ok(DepotSale {
            stock,
            item: request.item,
            count: request.count,
            proceeds,
        })
    }

    pub(super) fn sale_unit_credits(&self, cd: IntCompactDescr) -> Result<i64, ProgressionError> {
        let price = self
            .prices
            .prices
            .iter()
            .rev()
            .find(|(id, _)| *id == cd)
            .map(|(_, price)| price)
            .filter(|price| price.amount >= 0 && !self.prices.not_for_sale.contains(&cd))
            .ok_or(ProgressionError::NotForSale)?;
        let exchange_rate = match price.currency {
            "credits" => 1,
            "gold" if matches!(cd.item_type, item_type::SHELL | item_type::EQUIPMENT) => {
                AMMO_EXCHANGE_RATE
            }
            "gold" => GOLD_EXCHANGE_RATE,
            "crystal" => CRYSTAL_EXCHANGE_RATE,
            _ => return Err(ProgressionError::NotForSale),
        };
        let value = price
            .amount
            .checked_mul(exchange_rate)
            .ok_or(ProgressionError::InvalidRequest)?;
        Ok(value / SELL_PRICE_DIVISOR + i64::from(value % SELL_PRICE_DIVISOR != 0))
    }
}
