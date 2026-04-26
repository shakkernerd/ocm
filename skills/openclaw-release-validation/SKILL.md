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

1. Confirm paths, permissions, OCM binary, OpenClaw test repo, and report path.
2. Pull latest `origin/main` in the OpenClaw test repo and record the commit.
3. Build OpenClaw and use the built executable path, usually `openclaw.mjs`.
4. Review OCM command usage for the touched scenario using `README.md`,
   `docs/USAGE.md`, and targeted help:
   `ocm help start`, `ocm help service`, `ocm help logs`, `ocm help upgrade`,
   and `ocm help env`.
5. Review changelogs and commits since the last release to identify changed
   surfaces and extra tests.
6. Run the broad scenario matrix from
   `docs/OPENCLAW_RELEASE_SCENARIO_MATRIX.md`.
7. Copy existing user state before mutation; never mutate `Violet` directly.
8. For each scenario, test both clean new-user state and copied existing-user
   state when applicable.
9. For service scenarios, distinguish OCM-managed env services from OpenClaw's
   own service/gateway behavior; run OpenClaw commands through
   `ocm @<env> -- ...` unless the scenario explicitly requires direct execution.
   Verify OCM env runs expose `OPENCLAW_SERVICE_REPAIR_POLICY=external`.
10. Add extra checks for changed release surfaces, but keep them attached to the
   closest scenario.
11. Record a concise scenario table while testing; add detailed notes only for
   failures, blocked scenarios, or explicit gaps.
12. Clean up temp envs, services, LaunchAgents, plugin fixtures, and worktrees.

## Non-Negotiables

- Do not use the active `../openclaw` repo.
- Do not substitute `pnpm openclaw` for release validation of the built artifact.
- Do not mutate the real existing-user env; clone or copy it first.
- Do not leave temporary services or LaunchAgents running.
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

- Start with OCM/OpenClaw commits, baseline reviewed, temp paths, and state used.
- Include one summary row per scenario with New User, Existing User, Result, and
  Notes.
- For each failure, include impact, repro commands, expected, actual, and a
  short "Paste to fixer" summary.
- Avoid noisy successful command logs; keep command evidence in the report only
  where it proves a result or explains a failure.

When a user asks for release validation, run the matrix and update the report;
do not stop at a plan unless they explicitly ask for planning only.
