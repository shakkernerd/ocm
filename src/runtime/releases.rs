use super::RuntimeService;
use crate::releases::{load_release_manifest, query_releases};
use crate::types::RuntimeRelease;

impl<'a> RuntimeService<'a> {
    pub fn releases_from_manifest(
        &self,
        url: &str,
        version: Option<&str>,
        channel: Option<&str>,
    ) -> Result<Vec<RuntimeRelease>, String> {
        let manifest = load_release_manifest(url)?;
        query_releases(&manifest, version, channel)
    }
}
