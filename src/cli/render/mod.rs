pub mod env;
pub mod launcher;
pub mod runtime;

use std::collections::BTreeMap;

use time::OffsetDateTime;

pub(super) fn format_key_value_lines(lines: BTreeMap<String, String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect()
}

pub(super) fn format_rfc3339(value: OffsetDateTime) -> Result<String, String> {
    value
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| error.to_string())
}
