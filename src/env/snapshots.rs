use std::collections::BTreeMap;

use serde::Serialize;
use time::{Duration, OffsetDateTime};

use super::EnvironmentService;
use crate::store::{
    create_env_snapshot, get_env_snapshot, list_all_env_snapshots, list_env_snapshots, now_utc,
    remove_env_snapshot, restore_env_snapshot, summarize_snapshot,
};
use crate::supervisor::sync_supervisor_if_present;

#[derive(Clone, Debug)]
pub struct CreateEnvSnapshotOptions {
    pub env_name: String,
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshotSummary {
    pub id: String,
    pub env_name: String,
    pub label: Option<String>,
    pub archive_path: String,
    pub source_root: String,
    pub gateway_port: Option<u32>,
    pub service_enabled: bool,
    pub service_running: bool,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshotRestoreSummary {
    pub env_name: String,
    pub snapshot_id: String,
    pub label: Option<String>,
    pub root: String,
    pub archive_path: String,
    pub default_runtime: Option<String>,
    pub default_launcher: Option<String>,
    pub protected: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvSnapshotRemoveSummary {
    pub env_name: String,
    pub snapshot_id: String,
    pub label: Option<String>,
    pub archive_path: String,
}

#[derive(Clone, Debug)]
pub struct RestoreEnvSnapshotOptions {
    pub env_name: String,
    pub snapshot_id: String,
}

#[derive(Clone, Debug)]
pub struct RemoveEnvSnapshotOptions {
    pub env_name: String,
    pub snapshot_id: String,
}

pub fn select_snapshot_prune_candidates(
    snapshots: &[EnvSnapshotSummary],
    keep: Option<usize>,
    older_than_days: Option<i64>,
    now: OffsetDateTime,
) -> Vec<EnvSnapshotSummary> {
    let mut grouped = BTreeMap::<String, Vec<EnvSnapshotSummary>>::new();
    for snapshot in snapshots {
        grouped
            .entry(snapshot.env_name.clone())
            .or_default()
            .push(snapshot.clone());
    }

    let cutoff = older_than_days.map(|days| now - Duration::days(days));
    let keep = keep.unwrap_or(0);
    let mut out = Vec::new();

    for snapshots in grouped.values_mut() {
        sort_snapshots(snapshots);
        for (index, snapshot) in snapshots.iter().enumerate() {
            if index < keep {
                continue;
            }
            if let Some(cutoff) = cutoff
                && snapshot.created_at > cutoff
            {
                continue;
            }
            out.push(snapshot.clone());
        }
    }

    sort_snapshots(&mut out);
    out
}

fn sort_snapshots(snapshots: &mut [EnvSnapshotSummary]) {
    snapshots.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.id.cmp(&left.id))
    });
}

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
        let summary = restore_env_snapshot(options, self.env, self.cwd)?;
        sync_supervisor_if_present(self.env, self.cwd)?;
        Ok(summary)
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
        let snapshots = self.list_snapshots(env_name)?;
        Ok(select_snapshot_prune_candidates(
            &snapshots,
            keep,
            older_than_days,
            now_utc(),
        ))
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

#[cfg(test)]
mod tests {
    use super::{EnvSnapshotSummary, select_snapshot_prune_candidates};
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;

    #[test]
    fn snapshot_prune_selection_keeps_the_newest_snapshots_per_environment() {
        let now = parse_time("2026-03-25T22:00:00Z");
        let snapshots = vec![
            snapshot("alpha-3", "alpha", now - time::Duration::days(1)),
            snapshot("alpha-2", "alpha", now - time::Duration::days(2)),
            snapshot("alpha-1", "alpha", now - time::Duration::days(3)),
            snapshot("beta-2", "beta", now - time::Duration::days(1)),
            snapshot("beta-1", "beta", now - time::Duration::days(2)),
        ];

        let candidates = select_snapshot_prune_candidates(&snapshots, Some(1), None, now);
        let ids = candidates
            .iter()
            .map(|snapshot| snapshot.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["beta-1", "alpha-2", "alpha-1"]);
    }

    #[test]
    fn snapshot_prune_selection_respects_age_cutoffs_after_the_keep_floor() {
        let now = parse_time("2026-03-25T22:00:00Z");
        let snapshots = vec![
            snapshot("alpha-new", "alpha", now - time::Duration::days(1)),
            snapshot("alpha-old", "alpha", now - time::Duration::days(10)),
            snapshot("beta-kept", "beta", now - time::Duration::days(30)),
            snapshot("beta-old", "beta", now - time::Duration::days(40)),
        ];

        let candidates = select_snapshot_prune_candidates(&snapshots, Some(1), Some(7), now);
        let ids = candidates
            .iter()
            .map(|snapshot| snapshot.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, vec!["alpha-old", "beta-old"]);
    }

    fn snapshot(id: &str, env_name: &str, created_at: OffsetDateTime) -> EnvSnapshotSummary {
        EnvSnapshotSummary {
            id: id.to_string(),
            env_name: env_name.to_string(),
            label: None,
            archive_path: format!("/tmp/{id}.tar"),
            source_root: format!("/tmp/{env_name}"),
            gateway_port: None,
            service_enabled: false,
            service_running: false,
            default_runtime: None,
            default_launcher: None,
            protected: false,
            created_at,
        }
    }

    fn parse_time(raw: &str) -> OffsetDateTime {
        OffsetDateTime::parse(raw, &Rfc3339).unwrap()
    }
}
