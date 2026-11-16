use super::pe::Image;
use crate::binary_mapping::domain::{BaseClass, NativeType, Vtable};
use msvc_demangler::{DemangleFlags, demangle};
use std::collections::BTreeMap;

fn type_name(image: &Image<'_>, descriptor: u64) -> Option<(String, String)> {
    image
        .u64(descriptor)
        .filter(|pointer| image.slice(*pointer, 8).is_some())?;
    if image.u64(descriptor + 8)? != 0 {
        return None;
    }
    let decorated = image.string(descriptor + 16)?;
    if !decorated.starts_with(".?AV") && !decorated.starts_with(".?AU") {
        return None;
    }
    let name = demangle(
        &format!("??_R0{}@8", &decorated[1..]),
        DemangleFlags::NAME_ONLY,
    )
    .unwrap_or_else(|_| decorated.clone())
    .trim_end_matches("::`RTTI Type Descriptor'")
    .to_owned();
    Some((decorated, name))
}

pub fn extract(image: &Image<'_>) -> Vec<NativeType> {
    let mut types = BTreeMap::<u64, NativeType>::new();
    let mut locators = BTreeMap::<u64, (u64, u32, u32)>::new();
    for section in image
        .sections
        .iter()
        .filter(|s| !s.executable && !s.writable)
    {
        for offset in (0..section.file_size.saturating_sub(23)).step_by(4) {
            let address = section.address + offset as u64;
            if image.u32(address) != Some(1)
                || image.u32(address + 20).map(|v| image.base + v as u64) != Some(address)
            {
                continue;
            }
            let Some(descriptor) = image.u32(address + 12).map(|v| image.base + v as u64) else {
                continue;
            };
            let Some((decorated_name, name)) = type_name(image, descriptor) else {
                continue;
            };
            let Some(hierarchy) = image.u32(address + 16).map(|v| image.base + v as u64) else {
                continue;
            };
            let Some(count) = image.u32(hierarchy + 8).filter(|n| (1..=4096).contains(n)) else {
                continue;
            };
            let Some(array) = image.u32(hierarchy + 12).map(|v| image.base + v as u64) else {
                continue;
            };
            let mut bases = Vec::new();
            let mut valid = true;
            for index in 0..count {
                let base = (|| {
                    let entry = image.base + image.u32(array + index as u64 * 4)? as u64;
                    let base_descriptor = image.base + image.u32(entry)? as u64;
                    let (_, name) = type_name(image, base_descriptor)?;
                    Some(BaseClass {
                        descriptor: base_descriptor,
                        name,
                        contained_bases: image.u32(entry + 4)?,
                        member_displacement: image.u32(entry + 8)? as i32,
                        vbtable_displacement: image.u32(entry + 12)? as i32,
                        displacement_inside_vbtable: image.u32(entry + 16)? as i32,
                        attributes: image.u32(entry + 20)?,
                    })
                })();
                if let Some(base) = base {
                    bases.push(base);
                } else {
                    valid = false;
                    break;
                }
            }
            if !valid || bases.first().map(|b| b.descriptor) != Some(descriptor) {
                continue;
            }
            types.entry(descriptor).or_insert(NativeType {
                decorated_name,
                name,
                descriptor,
                bases,
                vtables: Vec::new(),
            });
            locators.insert(
                address,
                (
                    descriptor,
                    image.u32(address + 4).unwrap(),
                    image.u32(address + 8).unwrap(),
                ),
            );
        }
    }
    for section in image
        .sections
        .iter()
        .filter(|s| !s.executable && !s.writable)
    {
        for offset in (0..section.file_size.saturating_sub(7)).step_by(8) {
            let slot = section.address + offset as u64;
            let Some((descriptor, object_offset, constructor_displacement)) =
                image.u64(slot).and_then(|v| locators.get(&v))
            else {
                continue;
            };
            let mut functions = Vec::new();
            for index in 1..=4096 {
                let Some(function) = image.u64(slot + index * 8).filter(|v| image.executable(*v))
                else {
                    break;
                };
                functions.push(function);
            }
            if functions.is_empty() {
                continue;
            }
            types.get_mut(descriptor).unwrap().vtables.push(Vtable {
                address: slot + 8,
                locator: image.u64(slot).unwrap(),
                object_offset: *object_offset,
                constructor_displacement: *constructor_displacement,
                functions,
            });
        }
    }
    types.into_values().collect()
}
