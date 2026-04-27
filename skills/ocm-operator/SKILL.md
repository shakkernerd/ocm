---
name: ocm-operator
description: Use OpenClaw Manager (OCM) safely and correctly for OpenClaw environment lifecycle work, runtime installation/builds, release-shaped local builds, upgrades, migration/adoption, cloning existing user state, service supervision, gateway logs, dev worktrees, plugin testing, model/provider smoke tests, TUI/dashboard checks, cleanup, and concise reporting. Use whenever Codex is asked to use OCM, manage OpenClaw envs/runtimes/services, test OpenClaw with OCM, reproduce existing-user or new-user flows, or avoid breaking user state while working with OpenClaw.
---

# OCM Operator

## Operating Model

Use OCM as the control plane for OpenClaw. Treat an OCM environment as the
isolation boundary: it owns `OPENCLAW_HOME`, state, config, workspace, gateway
port, runtime/launcher binding, and service policy.

Prefer OCM commands over raw OpenClaw commands whenever the task touches:

- env creation, cloning, migration, export/import, snapshots, or cleanup
- runtime install/build/verify/update/remove
- upgrades or release testing
- background gateway services
- logs, status, TUI/dashboard, plugin checks, model checks

Run OpenClaw inside an env with:

```sh
ocm @<env> -- <openclaw args>
```

Use `ocm env exec <env> -- <command>` only when you need a non-OpenClaw command
inside the env, such as `env`, `node`, or shell tooling.

## First Steps

1. Inspect current state before changing it:

```sh
ocm env list
ocm service status
ocm runtime list
```

2. Read help for the exact surface before using unfamiliar flags:

```sh
ocm help start
ocm help env
ocm help runtime
ocm help service
ocm help logs
ocm help upgrade
```

3. For detailed command patterns, read
`references/command-cookbook.md`.

4. For destructive operations, service boundaries, or real user state, read
`references/safety-and-state.md` before acting.

5. For this machine's common paths and release-test defaults, read
`references/local-paths.md`.

## Decision Guide

- **Start a normal runtime-backed env**: use `ocm start <env> --runtime <runtime>`
  or `ocm start <env> --version <version>`.
- **Start a fresh user env**: use `ocm start <env> --runtime <runtime>` first;
  use `--onboard` only when testing interactive onboarding specifically.
- **Use source-backed OpenClaw development**: use `ocm dev <env> --repo <repo>`.
  Do not use this as proof of release packaging.
- **Test release behavior from local OpenClaw source**: use
  `ocm runtime build-local <runtime> --repo <repo> --force`, then test envs
  against that runtime.
- **Test existing-user upgrade behavior**: clone the existing env, upgrade the
  clone, and never mutate the source env directly.
- **Run OpenClaw commands in an env**: use `ocm @<env> -- ...`.
- **Read logs**: use `ocm logs <env>`.
- **Manage background gateway state**: use `ocm service status/start/stop/restart`.
- **Clean up**: use `ocm env destroy <env> --yes`; this removes env snapshots,
  OCM service policy, and OCM-created dev worktrees.

## Safety Rules

- Treat important user envs such as `Violet` and `Shaks` as durable state.
  Clone first for testing unless the user explicitly asks to mutate them.
- Remove envs with `ocm env destroy`.
- Manage OCM env gateway lifecycle with `ocm service`.
- Remember OCM injects `OPENCLAW_SERVICE_REPAIR_POLICY=external` for env-run
  OpenClaw processes, so `openclaw doctor --fix` should repair normal env
  state but skip OpenClaw-owned service lifecycle repair.
- Do not treat `ocm dev`, `pnpm openclaw`, or a source worktree as release
  validation. For release behavior, build and use a package runtime.
- Clean up temporary envs, services, worktrees, tarballs, and retained
  simulation envs.
- If a command could change real user data, run read-only inspection or clone
  first, then report the exact command you plan to run if there is ambiguity.

## Verification Standard

Finish by exercising the relevant command path. Prefer these checks:

```sh
ocm env list
ocm service status <env>
ocm logs <env> --tail 100
ocm @<env> -- --version
ocm @<env> -- status
ocm @<env> -- doctor --fix
```

Add plugin/model/TUI checks when the task touches plugins, model auth/catalog,
onboarding, or gateway connectivity:

```sh
ocm @<env> -- plugins list
ocm @<env> -- plugins update --all --dry-run
ocm @<env> -- models list
ocm @<env> -- tui
```

## Reporting

Keep reports quiet and useful:

- lead with pass/fail/blocker status
- include the exact command for failures
- include concise evidence for passes
- separate OCM issues from OpenClaw runtime/package issues
- say which relevant checks were skipped
- include cleanup performed

Use this failure shape:

```md
Failure:
- Command: `<exact command>`
- Expected: <expected state>
- Actual: <observed state/output>
- Impact: <why it matters>
- Likely owner: OCM | OpenClaw | test setup | external dependency
- Paste to fixer: <short actionable summary>
```
