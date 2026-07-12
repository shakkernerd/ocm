# OCM Safety And State Rules

## Isolation

OCM envs are isolated by `OPENCLAW_HOME`. Treat the env root and its
`.openclaw` state as user data. Prefer OCM commands for changes to env state.

Use OCM to inspect paths:

```sh
ocm env show <env>
ocm env exec <env> -- env | rg 'OPENCLAW_HOME|OPENCLAW_STATE_DIR|OPENCLAW_CONFIG_PATH|OPENCLAW_SERVICE_REPAIR_POLICY|OCM_ACTIVE_ENV'
```

## Existing User State

For tests involving existing users:

1. Use a clone.
2. Run the full flow on the clone.
3. Keep the source env untouched.
4. Destroy the clone when done.

Clones keep durable auth/settings but clear sessions, logs, and backups. Treat
every clone as secret-bearing:

- keep its service stopped by default
- do not contact providers, channels, webhooks, browsers, or other external
  services without explicit authorization
- use mocks, dedicated test accounts, or a credential-free fresh env for
  networked and untrusted-plugin checks
- do not export, publish, attach, or paste raw clone config or auth data

When the test must preserve sessions or history, copy the source env's
`.openclaw` directory into a task-owned temporary root and use `ocm adopt
import` on that copy. Verify the expected fixture exists before mutation.

Pattern:

```sh
ocm env clone <existing-env> <test-env>
ocm upgrade <test-env> --runtime <runtime>
ocm @<test-env> -- doctor --fix
ocm env destroy <test-env> --yes
```

## Service Boundary

OCM owns service lifecycle for OCM-managed envs. OpenClaw doctor should not
install/start/repair OpenClaw's own service when run through OCM.

Verify the boundary:

```sh
ocm env exec <env> -- env | rg 'OPENCLAW_SERVICE_REPAIR_POLICY'
ocm @<env> -- doctor --fix
```

Expected env value:

```text
OPENCLAW_SERVICE_REPAIR_POLICY=external
```

Use OCM service commands:

```sh
ocm service status <env>
ocm service start <env>
ocm service stop <env>
ocm service restart <env>
```

Avoid raw OpenClaw service commands for OCM-managed envs unless the user asks to
test standalone OpenClaw service behavior.

## Destructive Commands

Use preview modes when possible:

```sh
ocm env destroy <env>
ocm upgrade <env> --dry-run
ocm upgrade simulate <env> --to <target> --scenario all
```

Use `--yes` only after confirming the target env is not a protected real user
env:

```sh
ocm env destroy <env> --yes
```

Use `--force` only when the user requested it or when destroying an intentional
temporary env that is protected by test setup.

## Release-Shaped Testing

Use built package runtimes for release behavior:

```sh
ocm runtime build-local <runtime> --repo /path/to/openclaw --force
ocm start <env> --runtime <runtime>
```

Do not use `pnpm openclaw`, `ocm dev`, or a source worktree as proof that a
published package will work. Those paths can hide packaging, bundled extension,
or runtime dependency issues.

## Logs And Long-Running Commands

Use env logs:

```sh
ocm logs <env> --tail 100
ocm logs <env> --stream error --tail 100
ocm logs <env> --follow
```

For stuck TUI/gateway symptoms:

```sh
ocm service status <env>
ocm logs <env> --stream error --tail 200
ps -axo pid,ppid,stat,command | rg 'openclaw|ocm|gateway|tui'
```

Stop follow commands before ending a task unless the user asked to keep them
running.

## Cleanup Checklist

Before finishing:

```sh
ocm env list
ocm service status
```

Destroy temp envs:

```sh
ocm env destroy <temp-env> --yes
```

Remove temp worktrees created by manual git commands:

```sh
git -C /path/to/openclaw worktree list
git -C /path/to/worktree status --short
git -C /path/to/openclaw worktree remove /path/to/worktree
```

Remove only a worktree whose path and run id prove it belongs to the current
task. If status is not clean, stop and inspect it. Use `--force` only after
separately confirming that the worktree is disposable and every change in it
belongs to the current task.

Remove temp archives/directories:

```sh
rm -rf /tmp/<known-temp-dir>
```

Do not remove shared runtimes unless they were created for the task or the user
asks for runtime cleanup.
