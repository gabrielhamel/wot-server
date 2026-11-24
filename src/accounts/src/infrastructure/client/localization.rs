use std::collections::BTreeMap;
use wot_resources::ResourceBundle;

pub(super) fn localized_names(fs: &ResourceBundle, path: &str) -> Option<BTreeMap<String, String>> {
    wot_platform::client_formats::gettext::localized_names(fs.read(path).ok()?)
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
