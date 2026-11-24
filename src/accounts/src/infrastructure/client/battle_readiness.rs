use super::GarageCatalog;
use crate::{
    AccountSnapshot, OwnedVehicle,
    contract::{BattleVehicle, BattleVehicleChoice, BattleVehicleUnavailable},
};
use std::collections::BTreeSet;

impl GarageCatalog {
    pub(crate) fn battle_vehicle_choices(
        &self,
        account: &AccountSnapshot,
    ) -> Vec<BattleVehicleChoice> {
        let occupied = occupied_crew_seats(account);
        account
            .vehicles
            .iter()
            .map(|owned| {
                let name = self.vehicle_catalog_entry(&owned.catalog_key).map_or_else(
                    || "Unknown vehicle".to_owned(),
                    |entry| entry.name.to_owned(),
                );
                BattleVehicleChoice {
                    id: owned.inventory_id,
                    name,
                    unavailable_reason: self
                        .check_battle_vehicle(owned, occupied.as_ref())
                        .err()
                        .map(|reason| reason.to_string()),
                }
            })
            .collect()
    }

    #[tracing::instrument(name = "accounts.admin_battle_vehicle", skip_all, fields(
        ddd.bounded_context.name = "accounts", user.id = account.id, input.inventory_id = inventory_id,
        output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty,
    ), err)]
    pub(crate) fn admin_battle_vehicle(
        &self,
        account: &AccountSnapshot,
        inventory_id: i64,
    ) -> anyhow::Result<Result<BattleVehicle, BattleVehicleUnavailable>> {
        let span = tracing::Span::current();
        let readiness = account
            .vehicles
            .iter()
            .find(|owned| owned.inventory_id == inventory_id)
            .ok_or(BattleVehicleUnavailable::Unavailable)
            .and_then(|owned| {
                self.check_battle_vehicle(owned, occupied_crew_seats(account).as_ref())
            });
        let result = match readiness {
            Ok(()) => match self.battle_vehicle(account, inventory_id) {
                Ok(vehicle) => vehicle.ok_or(BattleVehicleUnavailable::Unavailable),
                Err(error) => {
                    span.record("output.outcome", "projection_failed");
                    span.record("otel.status_code", "ERROR");
                    return Err(error);
                }
            },
            Err(reason) => Err(reason),
        };
        span.record(
            "output.outcome",
            result
                .as_ref()
                .map_or_else(|reason| reason.outcome(), |_| "ready"),
        );
        Ok(result)
    }

    fn check_battle_vehicle(
        &self,
        owned: &OwnedVehicle,
        occupied: Option<&BTreeSet<(i64, usize)>>,
    ) -> Result<(), BattleVehicleUnavailable> {
        use BattleVehicleUnavailable::*;
        let offer = self
            .keys
            .get(&owned.catalog_key)
            .and_then(|id| self.offers.get(id))
            .filter(|offer| offer.grantable())
            .ok_or(Unavailable)?;
        let catalog = self.vehicle(&offer.key).map_err(|_| Unavailable)?;
        let slots = catalog.crew_roles.len();
        let complete = slots > 0
            && occupied.map_or(owned.has_crew, |occupied| {
                (0..slots).all(|slot| occupied.contains(&(owned.inventory_id, slot)))
            });
        if !complete {
            return Err(CrewIncomplete);
        }
        Ok(())
    }
}

fn occupied_crew_seats(account: &AccountSnapshot) -> Option<BTreeSet<(i64, usize)>> {
    account.crew.as_ref().map(|crew| {
        crew.members
            .values()
            .filter(|member| member.dismissed_at.is_none())
            .filter_map(|member| member.seat.map(|seat| (seat.vehicle, seat.slot)))
            .collect()
    })
}
