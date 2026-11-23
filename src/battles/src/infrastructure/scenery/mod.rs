//! Arena-local history of native destructibles, grouped by map chunk.

use super::obstacles::{Breakage, FIRST_MODULE, TREE_PART};
use super::*;
use std::f32::consts::{PI, TAU};

mod wire;

#[derive(Default)]
pub(super) struct SceneryAreas(BTreeMap<u16, Area>);

#[derive(Default)]
struct Area {
    entity_id: u32,
    broken: BrokenObjects,
}

#[derive(Default)]
struct BrokenObjects {
    modules: Vec<[u8; 3]>,
    fragiles: Vec<[u8; 3]>,
    columns: Vec<[u8; 3]>,
    trees: Vec<[u8; 5]>,
}

impl BrokenObjects {
    fn is_empty(&self) -> bool {
        self.modules.is_empty()
            && self.fragiles.is_empty()
            && self.columns.is_empty()
            && self.trees.is_empty()
    }

    fn record_tree(&mut self, index: u16, pitch: f32, cause: BreakageCause) {
        let [high, low] = index.to_be_bytes();
        let [pitch_high, pitch_low] = pack_angle(pitch, u16::MAX).to_be_bytes();
        self.trees
            .push([high, low, pitch_high, pitch_low, cause.pack_fall()]);
    }

    fn record(&mut self, index: u16, part: u32, cause: BreakageCause) -> Option<Option<u32>> {
        let [high, low] = index.to_be_bytes();
        let shot = u8::from(matches!(cause, BreakageCause::Shot { .. }));
        let module = match part {
            1 => {
                self.fragiles.push([high, low, shot]);
                0
            }
            2 => {
                self.columns.push([high, low, cause.pack_fall()]);
                return Some(None);
            }
            FIRST_MODULE..=85 => {
                let module = part as u8;
                self.modules.push([high, low, module << 1 | shot]);
                module
            }
            _ => return None,
        };
        Some((shot != 0 && index <= 255).then_some(u32::from(index) << 8 | u32::from(module)))
    }
}

#[derive(Clone, Copy)]
pub(super) enum BreakageCause {
    Contact { yaw: f32, speed: f32 },
    Shot { yaw: f32 },
}

fn pack_angle(radians: f32, max: u16) -> u16 {
    ((radians + PI).rem_euclid(TAU) / TAU * f32::from(max)).round() as u16
}

impl BreakageCause {
    fn yaw(self) -> f32 {
        match self {
            Self::Contact { yaw, .. } | Self::Shot { yaw } => yaw,
        }
    }

    fn pack_fall(self) -> u8 {
        let speed = match self {
            Self::Contact { speed, .. } => ((speed / 2.5) as u8).min(3),
            Self::Shot { .. } => 0,
        };
        (pack_angle(self.yaw(), 63) as u8) << 2 | speed
    }
}

impl SceneryAreas {
    pub(super) fn assign(
        &mut self,
        scenery: &obstacles::Scenery,
        mut allocate: impl FnMut() -> u32,
    ) {
        for chunk in scenery.chunks() {
            self.0.entry(chunk).or_insert_with(|| Area {
                entity_id: allocate(),
                ..Area::default()
            });
        }
    }

    pub(super) fn snapshot(
        &self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        client: ParticipantId,
    ) -> Result<usize> {
        let mut delivered = 0;
        for (&chunk, area) in self.0.iter().filter(|(_, area)| !area.broken.is_empty()) {
            wire.create_scenery_area(delivery, client, chunk, area.entity_id, &area.broken)?;
            delivered += 1;
        }
        Ok(delivered)
    }
}

impl Battle {
    #[tracing::instrument(name = "battles.scenery", skip_all, fields(
        ddd.bounded_context.name = "battles", input.arena_type_id = self.arena_type,
        input.scenery.source = match cause { BreakageCause::Contact { .. } => "contact", BreakageCause::Shot { .. } => "shot" },
        input.scenery.fragments.count = breaks.len(),
        output.scenery.fragments.count = tracing::field::Empty,
        output.scenery.trees.count = tracing::field::Empty,
        output.scenery.chunks.count = tracing::field::Empty,
        output.scenery.explosion_refs.count = tracing::field::Empty,
        output.scenery.deliveries.count = tracing::field::Empty,
        output.outcome = tracing::field::Empty, otel.status_code = tracing::field::Empty
    ))]
    pub(super) fn publish_breakages(
        &mut self,
        delivery: &mut impl ArenaDelivery,
        wire: &ArenaWire,
        breaks: &[Breakage],
        cause: BreakageCause,
    ) -> Vec<u32> {
        let mut explosion = Vec::new();
        let mut changed = BTreeMap::new();
        let mut replicated = 0_usize;
        let mut trees = 0_usize;
        for &Breakage { native, part } in breaks {
            let Some(native) = native else { continue };
            let chunk = (native >> 16) as u16;
            let Some(area) = self.areas.0.get_mut(&chunk) else {
                continue;
            };
            let first_break = area.broken.is_empty();
            let record = if part == TREE_PART {
                self.obstacles
                    .tree_fall_pitch(&self.ground, native, cause.yaw())
                    .map(|pitch| {
                        area.broken.record_tree(native as u16, pitch, cause);
                        trees += 1;
                        None
                    })
            } else {
                area.broken.record(native as u16, part, cause)
            };
            if let Some(reference) = record {
                changed.entry(chunk).or_insert(first_break);
                replicated += 1;
                if let Some(reference) = reference {
                    explosion.push((native & 0xffff_0000) | reference);
                }
            }
        }
        let span = tracing::Span::current();
        let mut delivered = 0_usize;
        let mut failed = false;
        for (&peer, _) in self.players.iter().filter(|(_, player)| player.entered) {
            for (&chunk, &create) in &changed {
                let Some(area) = self.areas.0.get(&chunk) else {
                    continue;
                };
                let result = (|| -> Result<()> {
                    if create {
                        wire.create_scenery_area(
                            delivery,
                            peer,
                            chunk,
                            area.entity_id,
                            &BrokenObjects::default(),
                        )?;
                    }
                    wire.update_scenery_area(delivery, peer, area.entity_id, &area.broken)
                })();
                if let Err(error) = result {
                    failed = true;
                    error!(%error, chunk, participant = peer.0, "Scenery replication failed");
                    delivery.disconnect(peer);
                    break;
                }
                delivered += 1;
            }
        }
        span.record("output.scenery.fragments.count", replicated);
        span.record("output.scenery.trees.count", trees);
        span.record("output.scenery.chunks.count", changed.len());
        span.record("output.scenery.explosion_refs.count", explosion.len());
        span.record("output.scenery.deliveries.count", delivered);
        span.record(
            "output.outcome",
            if failed {
                "delivery_failed"
            } else if changed.is_empty() {
                "no_native_identity"
            } else {
                "published"
            },
        );
        if failed {
            span.record("otel.status_code", "ERROR");
        }
        explosion
    }
}
