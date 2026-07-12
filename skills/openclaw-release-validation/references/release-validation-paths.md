# Release Validation Paths

Default paths for this machine:

- OCM repo: `/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/ocm`
- OpenClaw source repo and clone-time read-only object cache:
  `/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/test-build`
- Per-run root: `/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/release-validation/<run-id>`
- Per-run Git metadata: `<run-root>/repo`
- Dissociate that per-run clone so it owns every object needed by the recorded
  commit.
- Detached OpenClaw worktree: `<run-root>/openclaw`
- Do not use active OpenClaw repo: `/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/openclaw`
- Existing user source env: `/Users/shakker/.ocm/envs/Violet`
- Report pattern: `<run-root>/ocm-openclaw-release-validation-<run-id>.md`

Preferred binaries:

- OCM: `<ocm repo>/target/debug/ocm`
- Package-shaped OpenClaw runtime: an OCM runtime named with `<run-id>`, built
  from `<run-root>/openclaw` with `"$OCM_BIN" runtime build-local`
- Direct OpenClaw executable: `<run-root>/openclaw/openclaw.mjs`, only for the
  S02 source-artifact boot smoke check

Existing-user fixture modes:

- Durable-state clone: `"$OCM_BIN" env clone`; keeps auth/settings but
  intentionally clears sessions, logs, and backups.
- Session-preserving fixture: copy the source `.openclaw` directory under
  `<run-root>`, then import that copy with `"$OCM_BIN" adopt import`.

Both modes are secret-bearing until credentials are removed or replaced. Keep
their services stopped and use mocks or dedicated test accounts for external
checks unless the run explicitly authorizes real access.

Canonical scenario matrix:

- `<ocm repo>/docs/OPENCLAW_RELEASE_SCENARIO_MATRIX.md`

OCM usage references:

- `<ocm repo>/README.md`
- `<ocm repo>/docs/USAGE.md`
