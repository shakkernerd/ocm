# ocm

OpenClaw Manager.

`ocm` is a Rust CLI for creating and managing isolated OpenClaw environments on a local machine.
The current product focus is simple: give each workflow its own disposable `OPENCLAW_HOME`, let you
activate or execute inside that environment predictably, and keep the resulting state easy to
inspect and remove.

It already works as a practical local control plane for:

- isolated OpenClaw environments
- shell activation
- command execution inside an environment
- named launcher definitions for running OpenClaw
- safe cleanup and pruning

It is not yet a full runtime installer. Today, `version` means "named launcher definition", not "installer-managed OpenClaw runtime".

## Why this exists

OpenClaw state is easier to reason about when it is isolated.

`ocm` solves the local workflow problems that show up when multiple tasks, branches, experiments, or
projects all compete for one shared global OpenClaw setup:

- state collisions between unrelated tasks
- hard-to-reproduce behavior caused by shared `~/.openclaw`
- tedious manual shell setup when switching contexts
- risky cleanup when old environments accumulate
- lack of a simple way to bind an environment to a specific OpenClaw launch command

The main design choice is to make each environment its own `OPENCLAW_HOME`. That gives every
environment a single disposable root and keeps teardown predictable.

## Current model

The current CLI has two primary user-facing concepts.

### Environments

An environment is a named isolated OpenClaw root.

For an environment named `demo`, the default root is:

```text
~/.ocm/envs/demo
```

Derived paths inside that root:

```text
<env-root>/
  .ocm-env.json
  .openclaw/
    openclaw.json
    workspace/
```

`ocm` treats that root as:

- `OPENCLAW_HOME`
- the boundary for OpenClaw state and config
- the unit that can be activated, executed in, protected, removed, or pruned

### Versions

The current `version` entity is a launcher definition.

A version stores:

- a name
- a command string
- an optional working directory
- an optional description

That makes it flexible enough to point at:

- a globally installed `openclaw`
- a packaged binary
- a local dev command
- a shell-based launcher workflow

Long term, this concept will likely be renamed to `launcher`, while real installer-managed
OpenClaw binaries become `runtime`. For now, the compatibility-first CLI surface is still `version`.
The CLI also now supports `launcher add/list/show/remove` as compatibility aliases for the same
stored objects. Storage remains version-based for now, and environment binding still uses
`--version` / `env set-version`.

## What works today

Environment commands:

- `env create`
- `env list`
- `env show`
- `env use`
- `env exec`
- `env run`
- `env set-version`
- `env protect`
- `env remove`
- `env prune`

Version commands:

- `version add`
- `version list`
- `version show`
- `version remove`

Launcher alias commands:

- `launcher add`
- `launcher list`
- `launcher show`
- `launcher remove`

The current implementation is covered by integration tests around path handling, JSON compatibility,
environment lifecycle flows, shell activation, command execution, validation, safety rails, and
child exit-code propagation.

## What it does not do yet

`ocm` does not yet:

- download or install OpenClaw runtimes
- manage release channels
- expose `runtime install` or `runtime list`
- provide `ocm init zsh|bash|fish`
- implement clone, snapshot, export/import, or status flows

The README below documents the working surface as it exists today.

## Build and run

Recommended local-dev usage:

```bash
cargo build
./target/debug/ocm help
```

Or use the wrapper:

```bash
./bin/ocm help
```

`bin/ocm` is a development wrapper that runs the local crate through Cargo and keeps help output and activation examples pointing at the repo-local command.

## Quickstart

Register a launcher:

```bash
./bin/ocm launcher add stable --command openclaw
```

Create an isolated environment bound to that launcher:

```bash
./bin/ocm env create refactor-a --version stable --port 19789
```

Activate it in your shell:

```bash
eval "$(./bin/ocm env use refactor-a)"
```

Run any command inside the environment:

```bash
./bin/ocm env exec refactor-a -- sh -lc 'printf "%s\n" "$OPENCLAW_HOME"'
```

Run OpenClaw through the environment's bound launcher:

```bash
./bin/ocm env run refactor-a -- onboard
./bin/ocm env run refactor-a -- gateway run --port 19789
```

Inspect the environment:

```bash
./bin/ocm env show refactor-a
./bin/ocm env list --json
```

Clean up safely:

```bash
./bin/ocm env protect refactor-a on
./bin/ocm env prune --older-than 7
./bin/ocm env remove refactor-a --force
```

## Common use cases

### 1. Keep project work isolated

Create one environment per project, branch, or task so OpenClaw state stays separate:

```bash
./bin/ocm version add stable --command openclaw
./bin/ocm env create proj-a --version stable
./bin/ocm env create proj-b --version stable
```

### 2. Run a local development launcher

If OpenClaw is run from a local checkout or wrapper, store that as a version:

```bash
./bin/ocm version add local-dev \
  --command 'cargo run --bin openclaw --' \
  --cwd /path/to/openclaw \
  --description "Run the local OpenClaw checkout"
```

Then bind an environment to it:

```bash
./bin/ocm env create sandbox --version local-dev
./bin/ocm env run sandbox -- onboard
```

`env run` executes the stored launcher command and appends arguments after `--`. If the version has
`--cwd`, the launcher runs from that directory.

### 3. Automate environment-aware scripts

The CLI provides human-readable output and JSON output where it matters:

```bash
./bin/ocm env list --json
./bin/ocm env show sandbox --json
./bin/ocm launcher list --json
./bin/ocm launcher show stable --json
./bin/ocm version list --json
./bin/ocm version show stable --json
./bin/ocm env prune --json
```

### 4. Clean up old disposable environments

Preview prune candidates first:

```bash
./bin/ocm env prune --older-than 14
```

Apply removal only when you are ready:

```bash
./bin/ocm env prune --older-than 14 --yes
```

## Command reference

### Environment commands

`env create <name> [--root <path>] [--port <port>] [--version <name>] [--protect]`

- Creates the environment root and OpenClaw directories.
- Writes an `.ocm-env.json` marker file into the root.
- Can bind a default launcher with `--version`.
- Can store a gateway port with `--port`.
- Can mark the environment protected with `--protect`.

`env list [--json]`

- Lists all known environments from `OCM_HOME`.

`env show <name> [--json]`

- Shows the derived paths and stored metadata for one environment.

`env use <name> [--shell zsh|bash|sh|fish]`

- Prints shell code to activate the environment.
- Intended usage:

```bash
eval "$(./bin/ocm env use demo)"
```

`env exec <name> -- <command...>`

- Runs an arbitrary command inside the environment.
- Injects the OpenClaw environment variables directly.
- Requires `--` before the command.
- Propagates the child exit code.

`env run <name> [--version <name>] -- <openclaw args...>`

- Resolves the environment's default version or an explicit `--version`.
- Runs the stored launcher command through a shell.
- Appends the arguments after `--` to that launcher command.
- Uses the version's stored `cwd` when present, otherwise the current directory.
- Requires `--` before launcher arguments.

`env set-version <name> <version|none>`

- Sets or clears the default launcher for the environment.

`env protect <name> <on|off>`

- Enables or disables protection against accidental removal.

`env remove <name> [--force]`

- Removes one environment.
- Refuses to remove protected environments unless `--force` is used.
- Refuses to delete roots missing `.ocm-env.json` unless `--force` is used.

`env prune [--older-than <days>] [--yes] [--json]`

- Selects unprotected environments older than N days.
- Uses `lastUsedAt` when present, otherwise `createdAt`.
- Defaults to `14` days.
- Previews candidates by default.
- Requires `--yes` to actually remove them.

### Version commands

`version add <name> --command "<launcher>" [--cwd <path>] [--description <text>]`

- Registers a launcher definition.
- `--cwd` is normalized to an absolute path.

`version list [--json]`

- Lists all stored launcher definitions.

`version show <name> [--json]`

- Shows one launcher definition.

`version remove <name>`

- Removes one launcher definition.

### Launcher alias commands

`launcher add <name> --command "<launcher>" [--cwd <path>] [--description <text>]`

- Alias for `version add`.
- Stores the same underlying version metadata.

`launcher list [--json]`

- Alias for `version list`.

`launcher show <name> [--json]`

- Alias for `version show`.

`launcher remove <name>`

- Alias for `version remove`.

## Environment behavior

When activating or executing inside an environment, `ocm` sets:

- `OPENCLAW_HOME`
- `OPENCLAW_STATE_DIR`
- `OPENCLAW_CONFIG_PATH`
- `OCM_ACTIVE_ENV`
- `OCM_ACTIVE_ENV_ROOT`
- `OPENCLAW_GATEWAY_PORT` when configured

It also explicitly unsets:

- `OPENCLAW_PROFILE`

That unset is intentional. It avoids profile-based collisions with the environment-specific state
layout.

## Storage layout

The main store root is `OCM_HOME`.

Default:

```text
~/.ocm
```

Current layout:

```text
OCM_HOME/
  envs/
    <name>.json
    <name>/
      .ocm-env.json
      .openclaw/
  versions/
    <name>.json
```

Path rules:

- environment and version names must start with an ASCII letter or number
- remaining characters may use letters, numbers, `.`, `_`, or `-`
- relative `OCM_HOME` values are resolved against the current working directory
- relative custom `--root` values are resolved against the current working directory
- relative version `--cwd` values are resolved against the current working directory

Metadata is stored as human-readable JSON.

Representative environment metadata:

```json
{
  "kind": "ocm-env",
  "name": "demo",
  "root": "/path/to/.ocm/envs/demo",
  "gatewayPort": 19789,
  "defaultVersion": "stable",
  "protected": false,
  "createdAt": "2026-03-26T06:34:45.060385Z",
  "updatedAt": "2026-03-26T06:34:45.060385Z",
  "lastUsedAt": null
}
```

Representative version metadata:

```json
{
  "kind": "ocm-version",
  "name": "stable",
  "command": "openclaw",
  "cwd": null,
  "description": null,
  "createdAt": "2026-03-26T06:34:45.060385Z",
  "updatedAt": "2026-03-26T06:34:45.060385Z"
}
```

## Safety rails

The current CLI deliberately avoids destructive behavior unless the environment looks like one that
`ocm` created and owns.

- Protected environments are not removable without `--force`.
- Environments missing `.ocm-env.json` are not removable without `--force`.
- `env prune` previews by default and requires `--yes` to apply changes.

## Development notes

Repository structure:

- `src/main.rs`: binary entrypoint
- `src/cli.rs`: command routing and process execution
- `src/store.rs`: JSON metadata and filesystem operations
- `src/paths.rs`: path derivation, normalization, naming, and `OCM_HOME` resolution
- `src/shell.rs`: shell activation rendering and environment injection
- `src/types.rs`: shared structs and summaries
- `tests/`: integration coverage for path, store, behavior, and validation flows
- `bin/ocm`: development wrapper

Useful commands:

```bash
cargo build
cargo test
./target/debug/ocm help
./bin/ocm help
```

If you are continuing implementation work in a fresh session, read `AGENTS.md`,
`docs/OCM_PRODUCT_DIRECTION.md`, `docs/CHATGPT_CONTEXT.md`, and `docs/HANDOFF.md`.

## Direction

The next product step is not to replace the current model, but to extend it.

The intended direction is:

- keep environments as the main isolation unit
- preserve `version` compatibility while clarifying it as a launcher concept
- add real runtime management separately
- expand lifecycle and shell integration on top of the current foundation
