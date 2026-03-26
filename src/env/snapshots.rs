use super::EnvironmentService;
use crate::store::{
    create_env_snapshot, get_env_snapshot, list_all_env_snapshots, list_env_snapshots, now_utc,
    remove_env_snapshot, restore_env_snapshot, select_snapshot_prune_candidates,
    summarize_snapshot,
};
use crate::types::{
    CreateEnvSnapshotOptions, EnvSnapshotRemoveSummary, EnvSnapshotRestoreSummary,
    EnvSnapshotSummary, RemoveEnvSnapshotOptions, RestoreEnvSnapshotOptions,
};

impl<'a> EnvironmentService<'a> {
    pub fn create_snapshot(
        &self,
        options: CreateEnvSnapshotOptions,
    ) -> Result<EnvSnapshotSummary, String> {
        let meta = create_env_snapshot(options, self.env, self.cwd)?;
        Ok(summarize_snapshot(&meta))
    }

    pub fn list_snapshots(
        &self,
        env_name: Option<&str>,
    ) -> Result<Vec<EnvSnapshotSummary>, String> {
        let snapshots = match env_name {
            Some(env_name) => list_env_snapshots(env_name, self.env, self.cwd)?,
            None => list_all_env_snapshots(self.env, self.cwd)?,
        };
        Ok(snapshots.iter().map(summarize_snapshot).collect())
    }

    pub fn get_snapshot(
        &self,
        env_name: &str,
        snapshot_id: &str,
    ) -> Result<EnvSnapshotSummary, String> {
        let snapshot = get_env_snapshot(env_name, snapshot_id, self.env, self.cwd)?;
        Ok(summarize_snapshot(&snapshot))
    }

    pub fn restore_snapshot(
        &self,
        options: RestoreEnvSnapshotOptions,
    ) -> Result<EnvSnapshotRestoreSummary, String> {
        restore_env_snapshot(options, self.env, self.cwd)
    }

    pub fn remove_snapshot(
        &self,
        options: RemoveEnvSnapshotOptions,
    ) -> Result<EnvSnapshotRemoveSummary, String> {
        remove_env_snapshot(options, self.env, self.cwd)
    }

    pub fn prune_snapshot_candidates(
        &self,
        env_name: Option<&str>,
        keep: Option<usize>,
        older_than_days: Option<i64>,
    ) -> Result<Vec<EnvSnapshotSummary>, String> {
        let snapshots = match env_name {
            Some(env_name) => list_env_snapshots(env_name, self.env, self.cwd)?,
            None => list_all_env_snapshots(self.env, self.cwd)?,
        };
        let candidates =
            select_snapshot_prune_candidates(&snapshots, keep, older_than_days, now_utc());
        Ok(candidates.iter().map(summarize_snapshot).collect())
    }

    pub fn prune_snapshots(
        &self,
        env_name: Option<&str>,
        keep: Option<usize>,
        older_than_days: Option<i64>,
    ) -> Result<Vec<EnvSnapshotRemoveSummary>, String> {
        let candidates = self.prune_snapshot_candidates(env_name, keep, older_than_days)?;
        let mut removed = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            removed.push(remove_env_snapshot(
                RemoveEnvSnapshotOptions {
                    env_name: candidate.env_name,
                    snapshot_id: candidate.id,
                },
                self.env,
                self.cwd,
            )?);
        }
        Ok(removed)
    }
}
