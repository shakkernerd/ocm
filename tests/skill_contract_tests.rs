use std::fs;

fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn release_validation_requires_isolated_package_shaped_runs() {
    let skill = read("skills/openclaw-release-validation/SKILL.md");
    let paths = read("skills/openclaw-release-validation/references/release-validation-paths.md");
    let matrix = read("docs/OPENCLAW_RELEASE_SCENARIO_MATRIX.md");
    let combined = format!("{skill}\n{paths}\n{matrix}");
    let normalized = combined.to_lowercase();

    for required in [
        "ocm runtime build-local",
        "immutable",
        "detached worktree",
        "run id",
        "package-shaped runtime",
    ] {
        assert!(
            combined.contains(required),
            "release-validation contract must mention {required:?}"
        );
    }
    assert!(
        normalized.contains("only for the s02") || normalized.contains("limited to the s02"),
        "direct source execution must be limited to the S02 smoke check"
    );
}

#[test]
fn release_validation_distinguishes_session_and_secret_boundaries() {
    let release_skill = read("skills/openclaw-release-validation/SKILL.md");
    let operator_skill = read("skills/ocm-operator/SKILL.md");
    let cookbook = read("skills/ocm-operator/references/command-cookbook.md");
    let safety = read("skills/ocm-operator/references/safety-and-state.md");
    let matrix = read("docs/OPENCLAW_RELEASE_SCENARIO_MATRIX.md");
    let combined = format!("{release_skill}\n{operator_skill}\n{cookbook}\n{safety}\n{matrix}");
    let normalized = combined.to_lowercase();

    for required in [
        "secret-bearing",
        "explicit authorization",
        "dedicated test account",
        "ocm adopt import",
        "sessions, logs, and backups",
        "redact",
    ] {
        assert!(
            normalized.contains(required),
            "existing-user safety contract must mention {required:?}"
        );
    }
}

#[test]
fn operator_recipes_use_current_cli_and_safe_cleanup_contracts() {
    let usage = read("docs/USAGE.md");
    let cookbook = read("skills/ocm-operator/references/command-cookbook.md");
    let safety = read("skills/ocm-operator/references/safety-and-state.md");
    let paths = read("skills/ocm-operator/references/local-paths.md");
    let release_skill = read("skills/openclaw-release-validation/SKILL.md");
    let matrix = read("docs/OPENCLAW_RELEASE_SCENARIO_MATRIX.md");

    assert!(usage.contains("ocm logs mira --stream error"));
    assert!(!usage.contains("ocm logs mira --stderr"));
    assert!(cookbook.contains("ocm logs <env> --stream error"));
    assert!(safety.contains("git -C /path/to/worktree status --short"));
    assert!(!safety.contains("worktree remove --force"));
    assert!(paths.contains("set -euo pipefail"));
    assert!(paths.contains("status --porcelain"));
    assert!(paths.contains("rev-parse HEAD"));
    assert!(paths.contains("ocm_bin="));
    assert!(paths.contains("export OCM_HOME="));
    assert!(paths.contains("runtime which"));
    assert!(!paths.contains("$HOME/.ocm/runtimes"));
    assert!(release_skill.contains("OCM_BIN=<ocm-repo>/target/debug/ocm"));
    assert!(release_skill.contains("\"$OCM_BIN\" runtime build-local"));
    assert!(release_skill.contains("\"$OCM_BIN\" runtime verify"));
    assert!(release_skill.contains("\"$OCM_BIN\" runtime remove"));
    assert!(release_skill.contains("\"$OCM_BIN\" @<env> --"));
    assert!(matrix.contains("run-owned package runtime is removed"));
}
