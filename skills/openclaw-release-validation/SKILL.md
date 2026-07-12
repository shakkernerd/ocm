---
name: openclaw-release-validation
description: Validate OpenClaw release candidates through OCM using the built OpenClaw artifact, a broad reusable scenario matrix, copied existing-user state, clean new-user state, concise pass/fail reporting, and focused failure notes. Use when asked to test OpenClaw origin/main, release readiness, upgrade behavior, plugins, services, channels, doctor recovery, packaging, or broad release risk.
---

# OpenClaw Release Validation

## Overview

Run deep release validation for OpenClaw through OCM. Use the repo scenario
matrix as the source of truth, test built software instead of dev wrappers, and
produce a quiet report: quick pass/fail per scenario, with detail only for
failures, blocked cases, or release-risk notes.

## Required Reading

Before running tests, read:

- `../../README.md`
- `../../docs/USAGE.md`
- `references/release-validation-paths.md`
- `../../docs/OPENCLAW_RELEASE_SCENARIO_MATRIX.md`
- the current validation report if one exists under
  `/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/`

If repo-level AGENTS instructions are present, read and follow them first.

## Workflow

1. Confirm paths, permissions, OpenClaw source repo, and report root. Set
   `OCM_BIN=<ocm-repo>/target/debug/ocm`, verify it is executable, and invoke
   that absolute binary for every OCM command in the run.
2. Create a unique run id. Derive the worktree, runtime, env, report, and
   cleanup names from that id so concurrent validations cannot share mutable
   resources.
3. Fetch the OpenClaw source repo once, resolve `origin/main` to an immutable
   commit, and create a clean detached worktree for that commit. Record the
   commit before building.
4. Build the primary validation target with
   `"$OCM_BIN" runtime build-local <run-runtime> --repo <run-worktree>
   --force`, then run `"$OCM_BIN" runtime verify <run-runtime>`. This
   package-shaped runtime is the target for the matrix. Direct
   `<run-worktree>/openclaw.mjs` execution is limited to the S02
   source-artifact boot smoke check.
5. Review OCM command usage for the touched scenario using `README.md`,
   `docs/USAGE.md`, and targeted help from the same `"$OCM_BIN"`:
   `help start`, `help service`, `help logs`, `help upgrade`, and `help env`.
6. Review changelogs and commits since the last release to identify changed
   surfaces and extra tests.
7. Prepare existing-user fixtures before mutation:
   - use `"$OCM_BIN" env clone` only when clearing sessions, logs, and backups
     is intended;
   - for S20, copy the source env's `.openclaw` directory into the run root,
     import that copy with `"$OCM_BIN" adopt import`, and assert the expected
     session fixture exists before the upgrade.
8. Treat every copied or cloned user fixture as secret-bearing. Keep its
   service stopped and make no provider, channel, webhook, browser, or other
   external connection by default. Use a credential-free fresh env, mocks, or
   dedicated test accounts for networked checks. Real external access requires
   explicit authorization for the run.
9. Run the broad scenario matrix from
   `docs/OPENCLAW_RELEASE_SCENARIO_MATRIX.md`.
10. For each scenario, test both clean new-user state and copied existing-user
   state when applicable.
11. Run OpenClaw commands through `"$OCM_BIN" @<env> -- ...` unless the
    scenario explicitly requires direct execution. Verify OCM env runs expose
   `OPENCLAW_SERVICE_REPAIR_POLICY=external` as part of normal env execution;
   do not run broad LaunchAgent/service mutation tests unless the release
   changed service behavior.
12. Add extra checks for changed release surfaces, but keep them attached to the
   closest scenario.
13. Record a concise scenario table while testing; add detailed notes only for
   failures, blocked scenarios, or explicit gaps.
14. Before reporting, verify the detached worktree still points at the recorded
    commit and the runtime identity matches the run. Redact credentials,
    private endpoints, user identifiers, and secret-bearing command output.
15. Destroy run-owned envs, then run
    `"$OCM_BIN" runtime remove <run-runtime>`. Clean up only resources carrying
    the current run id. Inspect worktree status before removal and do not
    force-remove an unclean or unowned worktree.

## Non-Negotiables

- Do not use the active `../openclaw` repo.
- Do not substitute `pnpm openclaw` or the source-tree executable for
  validation of the package-shaped runtime.
- Do not mutate the real existing-user env; copy it into the run root first.
- Do not assume `"$OCM_BIN" env clone` preserves sessions, logs, or backups.
- Do not start a copied user's gateway or make external requests with retained
  credentials unless the run explicitly authorizes the destination and
  account.
- Do not include credentials or raw secret-bearing config in the report.
- Do not leave temporary services or LaunchAgents running.
- Do not leave the per-run package runtime installed after its envs are
  destroyed.
- Do not clean up resources that are not owned by the current run id.
- Do not mark a scenario passed unless postconditions were checked.
- Do not rely on reasoning alone for release readiness; run real command flows.

## Reporting

Use these statuses:

- `pass`: command path completed and postconditions were checked
- `fail`: reproducible breakage with exact command and output recorded
- `blocked`: could not run, with blocker and next action recorded
- `needs coverage`: important case not yet exercised in this run
- `n/a`: scenario does not apply to that user type

Report shape:

- Start with OCM/OpenClaw commits, package runtime identity, baseline reviewed,
  run id, temp paths, and fixture modes used.
- Include one summary row per scenario with New User, Existing User, Result, and
  Notes.
- For each failure, include impact, repro commands, expected, actual, and a
  short "Paste to fixer" summary.
- Avoid noisy successful command logs; keep command evidence in the report only
  where it proves a result or explains a failure.

When a user asks for release validation, run the matrix and update the report;
do not stop at a plan unless they explicitly ask for planning only.
