use super::{Account, ClientKey, EntityKey, Gateway};
use crate::{
    accounts::contract::{
        CosmeticChange, MAX_COSMETIC_PURCHASE_LINES, ProgressionCommand, cosmetic_item,
        decode_outfits,
    },
    player_sessions::mercury::app::entity::Bytes,
};

pub(super) const BUY_CUSTOMIZATIONS: i16 = 118;
pub(super) const APPLY_OUTFIT: i16 = 119;

impl Gateway {
    #[tracing::instrument(
        name = "game.cosmetics.buy",
        skip_all,
        fields(
            ddd.bounded_context.name = "player_sessions",
            input.request_id = request_id,
            input.shop_revision = args.first().copied(),
            input.inventory_id = args.get(1).copied(),
            input.args.count = args.len(),
        ),
        err,
    )]
    pub(super) fn buy_cosmetics(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        args: &[i32],
    ) -> anyhow::Result<()> {
        let command = parse_purchase(args);
        self.change_progression(
            client,
            entity,
            request_id,
            command,
            args.first().copied().map(i64::from),
        )
    }

    #[tracing::instrument(
        name = "game.cosmetics.apply",
        skip_all,
        fields(
            ddd.bounded_context.name = "player_sessions",
            input.request_id = request_id,
            input.shop_revision = ints.first().copied(),
            input.inventory_id = ints.get(1).copied(),
            input.args.count = ints.len(),
            input.outfits.count = strings.len(),
            input.outfit.bytes = strings.iter().map(|s| s.0.len()).sum::<usize>(),
        ),
        err,
    )]
    pub(super) fn apply_outfit(
        &mut self,
        client: ClientKey,
        entity: EntityKey<Account>,
        request_id: i16,
        ints: &[i64],
        strings: &[Bytes],
    ) -> anyhow::Result<()> {
        let command = (|| {
            let &[_, inventory_id, ref seasons @ ..] = ints else {
                return None;
            };
            let snapshot = &self.sessions.get(&client)?.account.snapshot;
            let default_emblem = self.catalog.default_vehicle_emblem(snapshot, inventory_id);
            let layouts = decode_outfits(
                seasons,
                strings.iter().map(|s| s.0.as_slice()),
                default_emblem,
            )?;
            Some(ProgressionCommand::Cosmetics {
                inventory_id,
                change: CosmeticChange::Apply(layouts),
            })
        })();
        self.change_progression(client, entity, request_id, command, ints.first().copied())
    }
}

fn parse_purchase(args: &[i32]) -> Option<ProgressionCommand> {
    let &[_, inventory_id, ref args @ ..] = args else {
        return None;
    };
    let (lines, remainder) = args.as_chunks::<2>();
    if !remainder.is_empty() || !(1..=MAX_COSMETIC_PURCHASE_LINES).contains(&lines.len()) {
        return None;
    }
    let items = lines
        .iter()
        .map(|&[item, count]| Some((cosmetic_item(item)?, u32::try_from(count).ok()?)))
        .collect::<Option<Vec<_>>>()?;
    Some(ProgressionCommand::Cosmetics {
        inventory_id: i64::from(inventory_id),
        change: CosmeticChange::Buy(items),
    })
}
