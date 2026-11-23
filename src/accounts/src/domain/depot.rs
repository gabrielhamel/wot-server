use super::{AccountSnapshot, ProgressionError, equipment::credit_disassembly};

pub struct DepotSale {
    pub stock: DepotStock,
    pub item: i32,
    pub count: u32,
    pub proceeds: DepotProceeds,
}

pub enum DepotProceeds {
    Credits(i64),
    Components(i64),
}

pub enum DepotStock {
    Shells,
    Fittings,
}

impl AccountSnapshot {
    #[tracing::instrument(name = "accounts.depot.sell_depot_stock", skip_all, fields(ddd.bounded_context.name = "accounts", user.id = self.id, input.stock.shell_types = self.depot_shells.len(), input.stock.fitting_types = self.progression.depot.len(), input.lines = sales.len()), err)]
    pub fn sell_depot_stock(&mut self, sales: Vec<DepotSale>) -> Result<(), ProgressionError> {
        let mut shells = self.depot_shells.clone();
        let mut fittings = self.progression.depot.clone();
        let mut balances = self.balances.clone();
        for sale in sales {
            if sale.count == 0 {
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
            match sale.proceeds {
                DepotProceeds::Credits(unit_value) => {
                    balances.credits = unit_value
                        .checked_mul(i64::from(sale.count))
                        .filter(|&proceeds| proceeds >= 0)
                        .and_then(|proceeds| balances.credits.checked_add(proceeds))
                        .ok_or(ProgressionError::InvalidRequest)?;
                }
                DepotProceeds::Components(unit_value) => {
                    credit_disassembly(&mut balances, sale.item, sale.count, unit_value)?;
                }
            }
        }
        self.depot_shells = shells;
        self.progression.depot = fittings;
        self.balances = balances;
        Ok(())
    }
}
