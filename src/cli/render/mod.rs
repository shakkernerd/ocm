pub mod env;
pub mod launcher;
pub mod runtime;
pub mod service;

use std::collections::BTreeMap;

use time::OffsetDateTime;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderProfile {
    pub pretty: bool,
    pub color: bool,
}

impl RenderProfile {
    pub fn raw() -> Self {
        Self {
            pretty: false,
            color: false,
        }
    }

    pub fn pretty(color: bool) -> Self {
        Self {
            pretty: true,
            color,
        }
    }
}

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
