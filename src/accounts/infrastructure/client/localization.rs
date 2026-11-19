use std::{collections::BTreeMap, io::Read};
use wot_resources::ResourceBundle;

pub(super) fn localized_names(fs: &ResourceBundle, path: &str) -> Option<BTreeMap<String, String>> {
    let mut bytes = Vec::new();
    fs.read(path)
        .ok()?
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let little_endian = match bytes.get(..4)? {
        [0xde, 0x12, 0x04, 0x95] => true,
        [0x95, 0x04, 0x12, 0xde] => false,
        _ => return None,
    };
    let word = |offset: usize| {
        let value: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
        Some(if little_endian {
            u32::from_le_bytes(value)
        } else {
            u32::from_be_bytes(value)
        } as usize)
    };
    let count = word(8)?;
    let keys = word(12)?;
    let translations = word(16)?;
    let string = |table: usize, index: usize| {
        let entry = table.checked_add(index.checked_mul(8)?)?;
        let length = word(entry)?;
        let start = word(entry.checked_add(4)?)?;
        std::str::from_utf8(bytes.get(start..start.checked_add(length)?)?).ok()
    };
    let mut names = BTreeMap::new();
    for index in 0..count.min(bytes.len() / 8) {
        if let (Some(key), Some(translation)) = (string(keys, index), string(translations, index))
            && !key.is_empty()
            && !translation.is_empty()
            && !translation.contains('\0')
        {
            names.insert(key.to_owned(), translation.to_owned());
        }
    }
    Some(names)
}

pub(super) fn humanize_item_name(name: &str) -> String {
    let mut result = String::new();
    let mut previous_lowercase = false;
    for ch in name.chars() {
        if ch == '_' {
            result.push(' ');
        } else if result.is_empty() {
            result.extend(ch.to_uppercase());
        } else {
            if previous_lowercase && ch.is_ascii_uppercase() {
                result.push(' ');
            }
            result.push(ch);
        }
        previous_lowercase = ch.is_ascii_lowercase();
    }
    result
}
