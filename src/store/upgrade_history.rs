use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::common::{load_json_files, path_exists, read_json, write_json};
use super::layout::{upgrade_history_env_dir, upgrade_history_meta_path, validate_name};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeHistoryBinding {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub openclaw_version: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeHistoryServiceState {
    pub enabled: bool,
    pub running: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeHistoryStage {
    pub status: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeHistoryRuntimeRecovery {
    pub runtime_name: String,
    #[serde(default)]
    pub release_version: Option<String>,
    #[serde(default)]
    pub backup_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpgradeHistoryRecord {
    pub kind: String,
    pub format_version: u32,
    pub id: String,
    pub env_name: String,
    pub source: UpgradeHistoryBinding,
    pub target: UpgradeHistoryBinding,
    pub snapshot_id: String,
    #[serde(default)]
    pub runtime_recovery: Vec<UpgradeHistoryRuntimeRecovery>,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub completed_at: OffsetDateTime,
    pub outcome: String,
    pub migration: UpgradeHistoryStage,
    pub finalization: UpgradeHistoryStage,
    pub service_before: UpgradeHistoryServiceState,
    pub service_after: UpgradeHistoryServiceState,
    #[serde(default)]
    pub rollback: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

pub fn save_upgrade_history_record(
    record: &UpgradeHistoryRecord,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<(), String> {
    let env_name = validate_name(&record.env_name, "Environment name")?;
    let transaction_id = validate_name(&record.id, "Upgrade transaction id")?;
    if record.kind != "ocm-upgrade-transaction" {
        return Err(format!("unsupported upgrade history kind: {}", record.kind));
    }
    if record.format_version != 1 {
        return Err(format!(
            "unsupported upgrade history format version: {}",
            record.format_version
        ));
    }
    let path = upgrade_history_meta_path(&env_name, &transaction_id, env, cwd)?;
    write_json(&path, record)
}

pub fn get_upgrade_history_record(
    env_name: &str,
    transaction_id: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<UpgradeHistoryRecord, String> {
    let env_name = validate_name(env_name, "Environment name")?;
    let transaction_id = validate_name(transaction_id, "Upgrade transaction id")?;
    let path = upgrade_history_meta_path(&env_name, &transaction_id, env, cwd)?;
    if !path_exists(&path) {
        return Err(format!(
            "upgrade transaction \"{transaction_id}\" does not exist for environment \"{env_name}\""
        ));
    }
    read_json(&path)
}

pub fn list_upgrade_history(
    env_name: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<Vec<UpgradeHistoryRecord>, String> {
    let env_name = validate_name(env_name, "Environment name")?;
    let dir = upgrade_history_env_dir(&env_name, env, cwd)?;
    let files = load_json_files(&dir)?;
    let mut records: Vec<UpgradeHistoryRecord> = Vec::with_capacity(files.len());
    for file in files {
        records.push(read_json(&file)?);
    }
    records.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    Ok(records)
}

pub(crate) fn remove_upgrade_recovery_for_snapshot(
    env_name: &str,
    snapshot_id: &str,
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<(), String> {
    let env_name = validate_name(env_name, "Environment name")?;
    let recovery_parent = upgrade_history_env_dir(&env_name, env, cwd)?;
    if !recovery_parent.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&recovery_parent).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let file_type = entry.file_type().map_err(|error| error.to_string())?;
        let path = entry.path();
        if !file_type.is_dir() || !entry.file_name().to_string_lossy().ends_with(".recovery") {
            continue;
        }
        let Ok(recovery_snapshot_id) = std::fs::read_to_string(path.join("snapshot-id")) else {
            continue;
        };
        if recovery_snapshot_id.trim() == snapshot_id {
            std::fs::remove_dir_all(&path).map_err(|error| {
                format!(
                    "failed to remove upgrade recovery at {}: {error}",
                    path.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use time::OffsetDateTime;

    use super::{
        UpgradeHistoryBinding, UpgradeHistoryRecord, UpgradeHistoryRuntimeRecovery,
        UpgradeHistoryServiceState, UpgradeHistoryStage, get_upgrade_history_record,
        list_upgrade_history, save_upgrade_history_record,
    };

    fn test_env(label: &str) -> (PathBuf, BTreeMap<String, String>) {
        let root = std::env::temp_dir().join(format!(
            "ocm-upgrade-history-{label}-{}",
            OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let mut env = BTreeMap::new();
        env.insert(
            "OCM_HOME".to_string(),
            root.join("ocm").to_string_lossy().into_owned(),
        );
        (root, env)
    }

    fn record(id: &str, started_at: OffsetDateTime) -> UpgradeHistoryRecord {
        UpgradeHistoryRecord {
            kind: "ocm-upgrade-transaction".to_string(),
            format_version: 1,
            id: id.to_string(),
            env_name: "demo".to_string(),
            source: UpgradeHistoryBinding {
                kind: "runtime".to_string(),
                name: "stable".to_string(),
                openclaw_version: Some("2026.6.11".to_string()),
            },
            target: UpgradeHistoryBinding {
                kind: "runtime".to_string(),
                name: "2026.6.33".to_string(),
                openclaw_version: Some("2026.6.33".to_string()),
            },
            snapshot_id: "snapshot-1".to_string(),
            runtime_recovery: vec![UpgradeHistoryRuntimeRecovery {
                runtime_name: "stable".to_string(),
                release_version: Some("2026.6.11".to_string()),
                backup_id: None,
            }],
            started_at,
            completed_at: started_at,
            outcome: "switched".to_string(),
            migration: UpgradeHistoryStage {
                status: "completed".to_string(),
                note: None,
            },
            finalization: UpgradeHistoryStage {
                status: "completed".to_string(),
                note: None,
            },
            service_before: UpgradeHistoryServiceState {
                enabled: true,
                running: true,
            },
            service_after: UpgradeHistoryServiceState {
                enabled: true,
                running: true,
            },
            rollback: None,
            note: None,
        }
    }

    #[test]
    fn upgrade_history_round_trips_and_sorts_newest_first() {
        let (root, env) = test_env("round-trip");
        let cwd = root.as_path();
        let older = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let newer = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();

        save_upgrade_history_record(&record("1700000000-000000001", older), &env, cwd).unwrap();
        save_upgrade_history_record(&record("1800000000-000000001", newer), &env, cwd).unwrap();

        let records = list_upgrade_history("demo", &env, cwd).unwrap();
        assert_eq!(
            records
                .iter()
                .map(|record| record.id.as_str())
                .collect::<Vec<_>>(),
            vec!["1800000000-000000001", "1700000000-000000001"]
        );
        let loaded = get_upgrade_history_record("demo", "1700000000-000000001", &env, cwd).unwrap();
        assert_eq!(loaded.source.openclaw_version.as_deref(), Some("2026.6.11"));
        assert_eq!(loaded.target.openclaw_version.as_deref(), Some("2026.6.33"));

        let _ = std::fs::remove_dir_all(root);
    }
}
