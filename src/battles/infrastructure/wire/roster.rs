use super::*;

pub(super) const ROSTER_ENTITY_NODE: u16 = 2;

impl ArenaWire {
    pub(in super::super) fn roster_ready(
        &self,
        base: &mut base::App,
        client: ClientKey,
        count: usize,
        row: usize,
        ready: bool,
    ) -> Result<()> {
        let schema = base
            .dispatch()
            .entity_from_id(self.roster_component)
            .context("Missing roster component")?;
        let (index, roster) = schema
            .properties
            .iter()
            .enumerate()
            .find(|(_, property)| &*property.name == "vehiclesInfo")
            .context("Missing vehiclesInfo")?;
        let TyKind::Array(array) = unalias(&roster.ty) else {
            anyhow::bail!("vehiclesInfo is not an array");
        };
        let TyKind::Dict(fields) = unalias(&array.ty) else {
            anyhow::bail!("Roster entry is not a dictionary");
        };
        anyhow::ensure!(row < count, "Invalid roster row");
        let node = ROSTER_ENTITY_NODE + 2 + u16::try_from(index)?;
        let mut records = Vec::new();
        for (name, value) in [
            ("isAvatarReady", WireValue::UInt8(u8::from(ready))),
            ("__generation", WireValue::UInt32(if ready { 1 } else { 2 })),
        ] {
            let (field, def) = fields
                .properties
                .iter()
                .enumerate()
                .find(|(_, p)| &*p.name == name)
                .with_context(|| format!("Missing roster field {name}"))?;
            anyhow::ensure!(
                def.ty.is_instance(&value),
                "Unexpected roster field type for {name}"
            );
            let mut path = pack_path([
                (1, 1),
                (index, index_width(schema.properties.len())),
                (1, 1),
                (row, index_width(count)),
                (0, 1),
                (field, index_width(fields.properties.len())),
            ]);
            value.write(&mut path, &def.ty)?;
            let mut record = node.to_be_bytes().to_vec();
            record.push(2);
            record.write_blob_variable(&path)?;
            records.push(record);
        }
        for record in records {
            base.send_arena_element(client, NrlData(record))?;
            base.send_arena_element(client, ApplyNrlUpdate {})?;
        }
        Ok(())
    }
}

fn index_width(count: usize) -> u32 {
    usize::BITS - count.saturating_sub(1).leading_zeros()
}

fn pack_path(fields: [(usize, u32); 6]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut current = 0;
    let mut used = 0;
    for (value, width) in fields {
        for shift in (0..width).rev() {
            current |= (((value >> shift) & 1) as u8) << (7 - used);
            used += 1;
            if used == 8 {
                bytes.push(current);
                current = 0;
                used = 0;
            }
        }
    }
    if used != 0 {
        bytes.push(current);
    }
    bytes
}
