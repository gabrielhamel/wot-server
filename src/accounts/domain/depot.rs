use super::{AccountSnapshot, ProgressionError};

pub struct DepotSale {
    pub stock: DepotStock,
    pub item: i32,
    pub count: u32,
    pub unit_credits: i64,
}

pub enum DepotStock {
    Shells,
    Fittings,
}

impl AccountSnapshot {
    #[tracing::instrument(name = "accounts.depot.sell_depot_stock", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = self.id, input.stock.shell_types = self.depot_shells.len(), input.stock.fitting_types = self.progression.depot.len()), err)]
    pub fn sell_depot_stock(
        &mut self,
        sales: impl IntoIterator<Item = DepotSale>,
    ) -> Result<(), ProgressionError> {
        let mut shells = self.depot_shells.clone();
        let mut fittings = self.progression.depot.clone();
        let mut credits = self.balances.credits;
        for sale in sales {
            if sale.count == 0 || sale.unit_credits < 0 {
                return Err(ProgressionError::InvalidRequest);
            }
            let stock = match sale.stock {
                DepotStock::Shells => &mut shells,
                DepotStock::Fittings => &mut fittings,
            };
            let remaining = stock
                .get(&sale.item)
                .and_then(|&count| count.checked_sub(sale.count))
                .ok_or(ProgressionError::InsufficientDepotStock)?;
            if remaining == 0 {
                stock.remove(&sale.item);
            } else {
                stock.insert(sale.item, remaining);
            }
            credits = sale
                .unit_credits
                .checked_mul(i64::from(sale.count))
                .and_then(|proceeds| credits.checked_add(proceeds))
                .ok_or(ProgressionError::InvalidRequest)?;
        }
        self.depot_shells = shells;
        self.progression.depot = fittings;
        self.balances.credits = credits;
        Ok(())
    }
}
