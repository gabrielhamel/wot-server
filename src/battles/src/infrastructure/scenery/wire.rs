use super::*;
use crate::infrastructure::wire::{properties_by_id, publish_entity};
use wot_platform::mercury::app::client::element::{SelectEntity, arena::ArenaEntityProperties};
use wot_platform::mercury::{app::dispatch::EntityDispatch, script::PropertyFlags};

impl ArenaWire {
    pub(in crate::infrastructure) fn validate_scenery(&self) -> Result<()> {
        let sample = BrokenObjects {
            modules: vec![[0, 0, 146]],
            fragiles: vec![[0, 1, 1]],
            columns: vec![[0, 2, 130]],
            trees: vec![[0, 3, 192, 0, 130]],
        };
        let schema = self.scenery_schema()?;
        area_properties(schema, &sample)?;
        for (name, value) in [
            ("destroyedModules", records(&sample.modules)),
            ("destroyedFragiles", records(&sample.fragiles)),
            ("fallenColumns", records(&sample.columns)),
            ("fallenTrees", records(&[[0; 5]])),
        ] {
            let property = schema
                .properties
                .iter()
                .find(|prop| prop.name.as_ref() == name)
                .with_context(|| format!("Missing AreaDestructibles property {name}"))?;
            anyhow::ensure!(
                property.flags == PropertyFlags::AllClients && property.ty.is_instance(&value),
                "Invalid AreaDestructibles property {name}"
            );
        }
        Ok(())
    }

    fn scenery_schema(&self) -> Result<&EntityDispatch> {
        self.dispatch
            .entity_from_id(self.area_type)
            .context("Missing AreaDestructibles")
    }

    pub(super) fn create_scenery_area(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        chunk: u16,
        entity_id: u32,
        broken: &BrokenObjects,
    ) -> Result<()> {
        let x = f32::from(chunk >> 8) - 127.0;
        let z = f32::from(chunk & 255) - 127.0;
        let (sin, cos) = (PI / 10.0 * (x + z)).sin_cos();
        publish_entity(
            delivery,
            client,
            CreateArenaEntity {
                entity_id,
                type_id: self.area_type,
                position: Vec3::new(
                    x * 100.0 + 50.0 + 4.5 * cos,
                    0.0,
                    z * 100.0 + 50.0 + 4.5 * sin,
                ),
                direction: Vec3::ZERO,
                properties: area_properties(self.scenery_schema()?, broken)?,
            },
        )?;
        Ok(())
    }

    pub(super) fn update_scenery_area(
        &self,
        delivery: &mut impl ArenaDelivery,
        client: ParticipantId,
        entity_id: u32,
        broken: &BrokenObjects,
    ) -> Result<()> {
        let update = area_properties(self.scenery_schema()?, broken)?;
        delivery.send_arena_element(client, SelectEntity { entity_id })?;
        delivery.send_arena_element(client, ArenaEntityProperties(update))?;
        Ok(())
    }
}

fn area_properties(schema: &EntityDispatch, broken: &BrokenObjects) -> Result<Vec<u8>> {
    properties_by_id(
        schema,
        [
            ("destroyedModules", records(&broken.modules)),
            ("destroyedFragiles", records(&broken.fragiles)),
            ("fallenColumns", records(&broken.columns)),
            ("fallenTrees", records(&broken.trees)),
        ],
    )
}

fn records<const N: usize>(rows: &[[u8; N]]) -> ScriptValue {
    ScriptValue::Seq(
        rows.iter()
            .map(|row| ScriptValue::Seq(row.iter().copied().map(ScriptValue::UInt8).collect()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use wot_platform::mercury::{net::codec::Codec, script};

    #[test]
    #[ignore = "Requires WG_RESOURCE_DIR with the extracted EU client bundle"]
    fn native_area_properties_encode_contact_and_shell_fractures() {
        let path = std::env::var_os("WG_RESOURCE_DIR").unwrap();
        let resources = ResourceBundle::open(std::path::Path::new(&path)).unwrap();
        let dispatch = ScriptDispatch::new(script::load(&resources).unwrap());
        let (_, schema) = dispatch.entity_from_name("AreaDestructibles").unwrap();
        let mut broken = BrokenObjects::default();
        let shot = BreakageCause::Shot { yaw: 0.0 };
        assert_eq!(broken.record(0x12, 73, shot), Some(Some(0x1249)));
        assert_eq!(broken.record(0x12, 75, shot), Some(Some(0x124b)));
        assert_eq!(broken.record(0x101, 1, shot), Some(None));
        assert_eq!(
            broken.record(
                5,
                2,
                BreakageCause::Contact {
                    yaw: 0.0,
                    speed: 5.5
                }
            ),
            Some(None)
        );
        broken.record_tree(
            0x0123,
            1.519_67,
            BreakageCause::Contact {
                yaw: 0.0,
                speed: 5.5,
            },
        );
        let expected = [
            ("destroyedModules", &[2, 0, 0x12, 147, 0, 0x12, 151][..]),
            ("destroyedFragiles", &[1, 1, 1, 1][..]),
            ("fallenColumns", &[1, 0, 5, 130][..]),
            ("fallenTrees", &[1, 1, 0x23, 189, 234, 130][..]),
        ];
        let encoded = area_properties(schema, &broken).unwrap();
        let mut data = encoded.as_slice();
        assert_eq!(data[0], 4);
        data = &data[1..];
        for (name, bytes) in expected {
            let property = &schema.properties[usize::from(data[0])];
            assert_eq!(property.name.as_ref(), name);
            data = &data[1..];
            assert!(data.starts_with(bytes));
            let mut payload = bytes;
            ScriptValue::read(&mut payload, &property.ty).unwrap();
            assert!(payload.is_empty());
            data = &data[bytes.len()..];
        }
        assert!(data.is_empty());
    }
}
