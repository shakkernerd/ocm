# OpenClaw Release Scenario Matrix

This is the durable scenario list for OpenClaw release validation through OCM.
Keep it broad and update it whenever OpenClaw, OCM, installer behavior, service
behavior, plugin behavior, channels, state layout, or release packaging changes.

The goal is simple: a new Codex session should read this file, test each
scenario against both clean new-user state and copied existing-user state where
applicable, then produce a quiet report that shows what passed and exactly what
failed.

## Operating Rules

- Use OCM from `/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/ocm`.
- Use the OpenClaw test repo at
  `/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/test-build`.
- Do not use `/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/openclaw`
  or `../openclaw`; that is an active working repo.
- Pull and test latest `origin/main` in `temp/test-build`.
- Test the built OpenClaw artifact, usually `<test-build>/openclaw.mjs`.
- Do not use `pnpm openclaw` as the main release-validation path.
- Use the local built OCM binary, usually `<ocm>/target/debug/ocm`.
- Copy existing user state, preferably `~/.ocm/envs/Violet`, into temp state
  before testing. Never mutate the real env.
- Keep the report quiet: summarize pass/fail per scenario, then include details
  only for failures, blocked scenarios, or release-risk notes.
- Clean up temp envs, services, LaunchAgents, generated fixtures, and worktrees.

## OCM Usage Primer For Test Agents

Read OCM's `README.md` and `docs/USAGE.md` before testing service, upgrade,
logs, start, clone, import, or runtime behavior. OCM is the env manager and
supervisor layer around OpenClaw; OpenClaw also has its own service/gateway
commands. Release validation must distinguish those layers instead of treating
every service side effect as an OCM bug.

Use OCM this way during validation:

- Create or bind envs with `ocm start`, `ocm dev`, `ocm env clone`,
  `ocm adopt`, and `ocm migrate` as appropriate.
- Run OpenClaw inside an env with `ocm @<env> -- <openclaw args>`.
- OCM env runs should inject `OPENCLAW_SERVICE_REPAIR_POLICY=external` so
  OpenClaw can report service health but does not mutate global/user-level
  service registrations from `doctor --fix`.
- Inspect env services with `ocm service status <env>`, `ocm service list`,
  `ocm service discover`, and `ocm logs <env>`.
- Simulate upgrades with `ocm upgrade simulate <env> --to <target>`.
- Use `ocm help start`, `ocm help service`, `ocm help logs`, `ocm help
  upgrade`, and `ocm help env` when the command shape is unclear.

Service terminology:

- OCM-managed service: background process policy/state owned by OCM for one
  environment.
- OpenClaw service/gateway: OpenClaw's own gateway/service behavior, invoked
  inside an OCM env through `ocm @<env> -- ...`.
- LaunchAgent: macOS service registration. A test may intentionally create one,
  but it must match the scenario and be cleaned up.

When validating service boundaries, test user intent:

- If an env was created without OCM service management, OCM should not mark it
  service-managed unless explicitly asked.
- If OpenClaw's own command or `doctor --fix` creates service state, report
  whether that state is expected OpenClaw behavior or an OCM isolation problem.
- OCM should not mistake a separate OpenClaw service outside OCM for the
  env-bound service under test.

## Report Format

Start each report with:

- OCM commit/version
- OpenClaw commit/version
- previous release or changelog baseline reviewed
- temp paths used
- whether real user state was copied and left untouched

Then use this summary table:

| ID | Scenario | New User | Existing User | Result | Notes |
| --- | --- | --- | --- | --- | --- |
| S01 | Built artifact boots | pass | pass | pass | concise note |

Use these statuses:

- `pass`: tested and postconditions were checked
- `fail`: reproducible breakage; add a detail section
- `blocked`: not run; explain blocker and next action
- `n/a`: not applicable for that user type
- `needs coverage`: important but not exercised in this run

For failures, add a short detail block:

```text
F01 - S12 Plugin uninstall removes installed files
Impact: installed plugin is still discovered after uninstall.
Repro: <exact commands>
Expected: config, ledger, and extension files are removed.
Actual: config and ledger removed, extension directory remains.
Paste to fixer: <short actionable summary>
```

Do not paste full logs unless they are needed. Include exact commands and the
smallest useful error/output snippet.

## Scenario List

### S01 - Release Diff Review

What to test:

- Identify the last release/tag used as the baseline.
- Read changelogs and commit summaries since that baseline.
- Add temporary extra checks for changed areas not already covered below.

Pass evidence:

- Report lists baseline, commit range, changed surfaces, and extra checks added.

### S02 - Built Artifact Boots

What to test:

- Pull latest `origin/main` in `temp/test-build`.
- Build the release artifact.
- Run the built executable directly with `--version`.
- Run the built executable through OCM-bound env execution.

Run for:

- new user: clean env bound to the built artifact
- existing user: copied env or clone bound to the built artifact

Pass evidence:

- Build passes, version prints expected commit/version, OCM runs that artifact.

### S03 - Release Packaging And Postinstall Shape

What to test:

- Published-package layout or local package build shape has expected entrypoints.
- Runtime files and sidecars expected by release users are present.
- Postinstall or package verification tests pass when available.

Run for:

- new user: install/start from release-shaped artifact
- existing user: upgrade simulation uses release-shaped artifact

Pass evidence:

- Package verification passes and no missing runtime entrypoints are found.

### S04 - Clean New User Start

What to test:

- Empty `HOME`, `OCM_HOME`, and OpenClaw state.
- `ocm start <env> --command <built-openclaw> --cwd <test-build>` creates a
  usable env.
- First run writes only expected minimum config/state.
- `openclaw --version`, `openclaw doctor`, `plugins list --json`, gateway
  status, and logs work.

Run for:

- new user only

Pass evidence:

- Env starts, diagnostics are clean, and no legacy or unexpected state appears.

### S05 - Existing User Upgrade From Copied State

What to test:

- Copy `Violet` into temp state.
- Import/adopt the copy into temp OCM state.
- Clone the copied env before destructive tests.
- Bind the clone to the built artifact.
- Run `ocm upgrade simulate <env> --to <test-build> --scenario current --json`.

Run for:

- existing user only

Pass evidence:

- Simulation passes, original copied env is not mutated, real `Violet` is not
  touched, and expected simulation steps are recorded.

### S06 - Upgrade Dry Run And Rollback Safety

What to test:

- `ocm upgrade --dry-run` previews without mutating env metadata, runtime
  bindings, snapshots, service policy, or files.
- Failed upgrade paths roll back or clearly report retained changes when
  rollback is disabled.
- Snapshot/backup references are usable.

Run for:

- existing user: copied env clone
- new user: clean env with a prior known-good binding

Pass evidence:

- Dry run leaves no mutation; failure either rolls back or reports explicit
  retained state.

### S07 - Runtime And Launcher Binding

What to test:

- Env resolves the intended runtime or launcher.
- `ocm @<env> -- <command>` uses the selected OpenClaw artifact.
- Binding changes are visible in `env show`, status, and JSON output.
- Clearing or replacing a binding does not leave stale execution paths.

Run for:

- new user
- existing user

Pass evidence:

- Effective command path is the built artifact and metadata remains coherent.

### S08 - Config Compatibility And Migration

What to test:

- Existing config files load without losing user settings.
- Legacy keys that still have migration support are migrated once.
- Removed or deprecated keys do not break startup.
- Config writes preserve unrelated user settings.

Run for:

- existing user: copied real state plus seeded legacy variants
- new user: clean config creation

Pass evidence:

- Config after command is valid, expected migrations occurred, and unrelated
  user settings remain unchanged.

### S09 - Doctor Read-Only Mode

What to test:

- `openclaw doctor` reports current state without mutating files, services, or
  install records.
- Missing dependency checks are accurate.
- Healthy release state does not show stale errors from prior releases.

Run for:

- new user
- existing user

Pass evidence:

- No file/service mutation in read-only mode and diagnostics match reality.

### S10 - Doctor Repair Mode

What to test:

- `openclaw doctor --fix` repairs supported issues.
- Repair is idempotent.
- Repair does not create unrelated default services or global state from an
  isolated test env.
- Follow-up read-only doctor is clean or reports only expected remaining issues.

Run for:

- new user with seeded repairable issues
- existing user with copied/legacy state

Pass evidence:

- Repair fixes targeted issues, second repair is a no-op, and no surprise
  LaunchAgent/service is left behind.

### S11 - Plugin Discovery And Registry Health

What to test:

- `plugins list --json` discovers bundled and installed plugins.
- Output has stable ids, enabled state, source, format, and diagnostics.
- Missing or malformed plugin residue is reported clearly.
- Healthy state has no diagnostics.

Run for:

- new user
- existing user

Pass evidence:

- Plugin list is deterministic and diagnostics are absent unless intentionally
  seeded.

### S12 - Plugin Install Compatibility

What to test:

- Valid native plugin directory with `openclaw.plugin.json`.
- Valid native plugin archive with `openclaw.plugin.json`.
- Valid Discord-style package with `openclaw.plugin.json` plus rich
  `package.json` metadata, dependencies, peer deps, compat/build fields, and
  package-level `openclaw` metadata.
- Valid Codex bundle/skill package if supported by the current release.
- Old package-json-only archive with `openclaw.extensions` but no
  `openclaw.plugin.json` fails cleanly.
- Malformed manifest, missing entry file, duplicate id, and manifest id
  different from npm package name.

Run for:

- new user
- existing user

Pass evidence:

- Valid packages install and list cleanly; invalid packages fail before writing
  config, ledger, or extension files.

### S13 - Plugin Lifecycle

What to test:

- Install, disable, enable, reinstall/upgrade, update dry-run, update all
  dry-run, uninstall dry-run, and uninstall force.
- Uninstall removes config, install ledger, and extension files.
- Path-sourced plugins are skipped or handled correctly by update commands.

Run for:

- new user
- existing user

Pass evidence:

- Final list no longer discovers uninstalled plugins and no stale diagnostics
  remain.

### S14 - Bundled Runtime Dependencies

What to test:

- Bundled plugin runtime dependencies are present in the built artifact.
- `pnpm test:build:bundled-runtime-deps` passes.
- `doctor` does not show missing bundled runtime dependency blocks on healthy
  builds.

Run for:

- new user
- existing user

Pass evidence:

- Dependency test passes and doctor output is clean for bundled deps.

### S15 - Gateway Lifecycle

What to test:

- Gateway status works before and after start.
- Explicit service start/stop/status works.
- Gateway binds to the expected env and port.
- Gateway failure leaves useful status/log evidence.

Run for:

- new user
- existing user

Pass evidence:

- Status points at the right env/binary and no stale/global gateway is mistaken
  for the env gateway.

### S16 - OpenClaw Service And OCM Service Boundaries

What to test:

- OCM-created envs respect the user's service intent: no OCM-managed background
  service unless the OCM command path requested one.
- OCM env execution and shell activation set
  `OPENCLAW_SERVICE_REPAIR_POLICY=external`.
- OpenClaw's own service/gateway behavior, including anything triggered by
  `openclaw doctor --fix`, is run through `ocm @<env> -- ...` so the active
  `OPENCLAW_HOME` is the test env.
- If OpenClaw creates or repairs a LaunchAgent, identify whether it is expected
  OpenClaw service behavior, unexpected behavior in an OCM-managed env, or OCM
  incorrectly leaking/defaulting service state.
- `ocm service status <env>`, `ocm service list`, `ocm service discover`, and
  `ocm logs <env>` reflect the OCM env being tested and do not confuse it with
  a separate OpenClaw service outside OCM.
- Service-enabled envs create only expected OCM service policy/state.
- Temp `ai.openclaw.gateway` or other test-created services are stopped/booted
  out.

Run for:

- new user
- existing user

Pass evidence:

- OCM service state matches user intent, env commands expose
  `OPENCLAW_SERVICE_REPAIR_POLICY=external`, OpenClaw service side effects are
  classified correctly, and cleanup leaves no temp LaunchAgent or stale service.

### S17 - Logs And Support Evidence

What to test:

- `ocm logs <env>` and relevant OpenClaw log commands show current env logs.
- JSON/raw log output is script-friendly.
- Failed startup or plugin errors are visible without digging through random
  files.

Run for:

- new user
- existing user

Pass evidence:

- Logs identify the current env and contain enough evidence to debug failures.

### S18 - Channel And Connector Smoke

What to test:

- Changed bundled channels/connectors load their manifests and config schemas.
- Channel-specific env vars and secret/config validation behave correctly.
- Telegram, Discord, Slack, MCP, browser, voice/video, TTS, or other changed
  channels do not break plugin discovery or doctor.

Run for:

- new user: clean config plus minimal channel setup when safe
- existing user: copied config with existing channel settings

Pass evidence:

- Changed channel surfaces load without diagnostics and preserve existing
  config.

### S19 - Model And Provider Configuration

What to test:

- Default model/provider changes are reflected in config and startup.
- Existing user provider settings are preserved.
- Missing provider credentials fail with clear diagnostics, not startup crashes.

Run for:

- new user
- existing user

Pass evidence:

- Defaults are sane for new users and existing provider choices survive upgrade.

### S20 - Browser/UI Build And Static Assets

What to test:

- UI build passes.
- Browser automation or UI assets touched by the release are present.
- Static assets referenced by the gateway/UI resolve after build.

Run for:

- new user
- existing user through upgrade simulation

Pass evidence:

- UI build passes and no missing asset/runtime errors appear in gateway logs.

### S21 - Sessions And Persistent State

What to test:

- Existing sessions, bindings, and conversation state can be read after upgrade.
- New sessions can be created in clean state.
- Session state paths stay under the intended OpenClaw home.

Run for:

- new user
- existing user

Pass evidence:

- Existing state remains readable and new state is written to the env root.

### S22 - Secrets And Environment Isolation

What to test:

- Env-scoped secrets/config are resolved from the copied or clean env only.
- Parent shell `OPENCLAW_*` or OCM variables do not leak into child envs.
- Missing secrets produce actionable diagnostics.

Run for:

- new user
- existing user

Pass evidence:

- Commands use the intended env root and no cross-env secret/config leakage is
  observed.

### S23 - Filesystem And Path Safety

What to test:

- Paths with spaces or symlinks work where supported.
- macOS `/private/tmp` normalization does not break env detection.
- Commands do not write outside temp state except intentional package caches.
- Destructive commands keep safety rails.

Run for:

- new user
- existing user

Pass evidence:

- State remains inside the intended roots and destructive commands require the
  expected confirmation/flags.

### S24 - Snapshot, Clone, Export, And Import

What to test:

- Env clone preserves usable state without sharing mutable directories.
- Snapshot/export/import round trips a test env.
- Imported env metadata points to valid local paths.

Run for:

- existing user: copied env
- new user: clean env after setup

Pass evidence:

- Round-tripped env runs the built artifact and original env is unchanged.

### S25 - Update Commands And Network Failure Behavior

What to test:

- Update commands handle available updates, no-op updates, path-sourced installs,
  and network failures.
- Network errors do not corrupt config or install records.

Run for:

- new user
- existing user

Pass evidence:

- No-op and failure paths leave state unchanged; actionable errors are shown.

### S26 - Release Notes Claims

What to test:

- Every user-facing release-note claim has at least one command-path check or is
  marked `needs coverage`.
- New features mentioned in release notes work for new users and do not break
  existing users.

Run for:

- new user
- existing user

Pass evidence:

- Report maps release-note claims to tested scenarios or explicit gaps.

### S27 - Cleanup Verification

What to test:

- Temp OCM homes, copied envs, generated fixtures, worktrees, and services are
  removed when no longer needed.
- Real `Violet` was not mutated.
- `temp/test-build` has no unintended worktree changes.

Run for:

- every validation run

Pass evidence:

- Cleanup section lists what was removed and any intentional leftovers.

## Known Risks To Keep Rechecking

- `plugins uninstall --force` must remove extension files, not only config and
  install ledger state.
- `doctor --fix` from isolated OCM-managed state must not install/start an
  unexpected default user-level `ai.openclaw.gateway` LaunchAgent.
- missing bundled plugin runtime dependencies must remain fixed after each pull.
- invalid package-json-only archives must fail cleanly without install residue.
- Discord-style packages with both canonical manifests and rich package metadata
  must remain installable.
