# Usage Guide

This guide explains how to use `ocm` in common situations and how the main command groups fit together.

If you are new to `ocm`, start with the [README](../README.md) first. It keeps the first-run path short. This guide goes deeper.

## What `ocm` manages

The main unit in `ocm` is the environment.

- `env`: one isolated OpenClaw environment
- `release`: one published OpenClaw release that you can inspect or install
- `runtime`: one local OpenClaw install managed by `ocm`
- `launcher`: one named command used to run OpenClaw
- `service`: one background OpenClaw process tied to one environment

In practice:

- use `env` every day
- use `release` and `runtime` when you want published OpenClaw versions
- use `launcher` when you want a local checkout or custom command
- use `service` when an environment should keep running in the background

## The quickest ways to get started

### Guided setup

Use this if you want `ocm` to walk you through the choices:

```bash
ocm setup
```

This is the easiest path for:

- first-time use
- choosing between stable, beta, exact version, or local checkout
- letting `ocm` suggest an environment name

### Fast start

Use this if you already know what you want:

```bash
ocm start
```

Or with a specific choice:

```bash
ocm start mira --channel beta
ocm start mira --version 2026.3.24
ocm start luna --command 'pnpm openclaw' --cwd /path/to/openclaw
```

`start` creates or reuses the environment, prepares the chosen OpenClaw source, and can run onboarding for you.
By default, it also installs and starts the environment service so OpenClaw keeps running in the background.

## Common scenarios

### 1. Use the latest stable release

```bash
ocm start
```

Or:

```bash
ocm start mira --channel stable
```

Use this when you want the normal supported path without worrying about local checkout details. `start` keeps the environment running in the background unless you pass `--no-service`.

### 2. Try the beta release

```bash
ocm start mira --channel beta
```

Use this when you want a published prerelease without moving to a local development build.

### 3. Pin to an exact published release

```bash
ocm start mira --version 2026.3.24
```

Use this when you need repeatability across machines or teams.

### 4. Run a local checkout

```bash
ocm start luna --command 'pnpm openclaw' --cwd /path/to/openclaw --no-service
```

Use this when you are developing OpenClaw locally or want a custom run command.

If you run `ocm setup` from inside an OpenClaw checkout, local mode can detect that and fill in sensible defaults.

### 5. Keep an environment running in the background

```bash
ocm start mira
ocm service status mira
```

`start` and `setup` already do this by default. Use `service install` directly when you skipped the background service earlier with `--no-service`.

### 6. Update OpenClaw later

```bash
ocm upgrade mira
ocm upgrade --all
ocm upgrade mira --dry-run
ocm upgrade simulate mira --to 2026.4.20
ocm upgrade simulate mira --to ./openclaw
```

Use this when a newer OpenClaw release is available and you want your environments to move forward without manually updating runtimes and restarting services.
Use `upgrade simulate` when you want to test what would happen against a published release or local OpenClaw repo before touching the real env.

`upgrade` is env-first:

- channel-tracked runtimes move forward
- simulations clone the source env, run OpenClaw's update dry-run plan for published targets, validate local repo builds for repo targets, then run update-mode doctor, plugin update dry-run, and gateway status checks
- a pre-upgrade snapshot is created before env state changes
- if service reconciliation fails, OCM restores the snapshot and previous runtime unless `--no-rollback` is set
- pinned runtimes stay pinned unless you pass `--version` or `--channel`
- local-command environments are reported clearly instead of being changed behind your back

### 7. Run OpenClaw without activating the shell first

```bash
ocm @mira -- status
ocm @mira -- tui
ocm @mira -- onboard
```

Use this for quick one-off runs.

### 8. Activate an environment in your current shell

```bash
eval "$(ocm env use mira)"
ocm -- status
```

Use this when you want to stay inside one environment for a longer interactive session.

## Choosing between releases, runtimes, and launchers

This is the part that usually needs the most explanation.

### Use `release` when you want to browse what is published

Examples:

```bash
ocm release list
ocm release list --channel stable
ocm release show --channel stable
ocm release show 2026.3.24
```

`release` does not run OpenClaw by itself. It tells you what is available to install.

### Use `runtime` when you want a local installed OpenClaw

Examples:

```bash
ocm release install --channel stable
ocm runtime list
ocm runtime show stable
ocm runtime verify stable
```

Use a runtime when you want:

- a published OpenClaw release installed locally
- stable naming like `stable` or `2026.3.24`
- verification and updates

### Use `launcher` when you want a command recipe

Examples:

```bash
ocm launcher add dev --command 'pnpm openclaw' --cwd /path/to/openclaw
ocm launcher list
ocm launcher show dev
```

Use a launcher when you want:

- a local checkout
- a wrapper script
- a custom command line
- something that is not just a published OpenClaw release

### The simple rule

- published OpenClaw release: use `release` and `runtime`
- local checkout or custom command: use `launcher`
- day-to-day work: use `env`

## Running commands inside environments

### Activate the environment

```bash
eval "$(ocm env use mira)"
```

After that:

```bash
ocm -- status
ocm -- onboard
```

This is the shortest interactive path.

### Run without activating

```bash
ocm @mira -- status
ocm @mira -- tui
ocm @mira -- onboard
```

This is the shortest non-interactive path.

### Run any other command in the environment

```bash
ocm env exec mira -- sh -lc 'echo "$OPENCLAW_HOME"'
```

## Service management

Use `service` when an environment should run in the background.

### Install and inspect

```bash
ocm service install mira
ocm service list
ocm service status mira
```

Use `service install` when you want a background service for an environment that was created with `--no-service`, or when you want to bring an older env under background management later.

### Read logs

```bash
ocm logs mira --tail 50
ocm logs mira --stderr
ocm logs mira --follow
```

### Start, stop, restart

```bash
ocm service start mira
ocm service stop mira
ocm service restart mira
```

### Remove the service

```bash
ocm service uninstall mira
```

This removes the background service. It does not remove the environment.

## Upgrading OpenClaw and `ocm`

### Upgrade one environment

```bash
ocm upgrade mira
```

This is the normal command when `mira` tracks a channel like `stable` or `beta`.
Use `--dry-run` to preview the transaction without writing snapshots, runtimes, envs, or services.

### Upgrade every environment that can be updated safely

```bash
ocm upgrade --all
```

This updates channel-tracked environments and restarts their running services when needed.

### Move a pinned or local env to a different published release

```bash
ocm upgrade mira --channel beta
ocm upgrade mira --version 2026.3.24
```

Use this when you want to deliberately move one environment to a different published release.

### Update `ocm` itself

```bash
ocm self update
ocm self update --check
```

### Discover other OpenClaw services on the machine

```bash
ocm service discover
```

Use this when you want to see:

- OCM-managed services
- a separate OpenClaw service outside OCM
- other discovered OpenClaw services on the machine

## Environment lifecycle

### Clone an environment

```bash
ocm env clone mira rowan
```

Clone copies the workspace and env config into a new environment, gives the clone its own gateway port, rewrites env-scoped OpenClaw config paths under the new env root, keeps durable agent auth/settings for the same user, clears copied runtime residue like sessions, logs, and backups, and keeps the background service separate. The usual next step is:

```bash
ocm start rowan
```

### Snapshots

Create:

```bash
ocm env snapshot create mira --label before-upgrade
```

List:

```bash
ocm env snapshot list mira
ocm env snapshot list --all
```

Show:

```bash
ocm env snapshot show mira <snapshot>
```

Restore:

```bash
ocm env snapshot restore mira <snapshot>
```

Remove:

```bash
ocm env snapshot remove mira <snapshot>
```

Prune:

```bash
ocm env snapshot prune mira --keep 3 --yes
```

### Export and import

Export:

```bash
ocm env export mira
```

Import:

```bash
ocm env import ./mira.tar --name rowan
```

Imported environments get a fresh identity, have env-scoped OpenClaw config rewritten for the new root, and keep durable agent settings while clearing copied runtime residue like sessions, logs, and backup files.

### Inspect and repair

```bash
ocm env status mira
ocm env doctor mira
ocm env cleanup mira --yes
ocm env repair-marker mira
```

### Remove or destroy

Remove only the environment:

```bash
ocm env remove mira
```

Destroy the environment, its OCM-managed service, and its snapshots:

```bash
ocm env destroy mira
ocm env destroy mira --yes
```

`destroy` is the stronger cleanup path.

## Updating `ocm` itself

Check for updates:

```bash
ocm self update --check
```

Update to the latest release:

```bash
ocm self update
```

Update to a specific version:

```bash
ocm self update --version 0.2.1
```

If you installed `ocm` long ago and the `self` command is not available yet, rerun the install script once with `--force`, then use `ocm self update` from then on.

## Output modes

By default, `ocm` formats output for people in a terminal.

Use:

- `--json` for structured output
- `--raw` for plain output
- `--color auto|always|never` for explicit color control

Examples:

```bash
ocm service list --json
ocm env status mira --raw
ocm --color always runtime list
```

## Command map

### Top-level commands

- `ocm setup`
- `ocm start`
- `ocm self`
- `ocm env`
- `ocm release`
- `ocm runtime`
- `ocm launcher`
- `ocm service`
- `ocm init`
- `ocm help`
- `ocm --version`

### `env`

- create, clone, list, show
- use, exec, run, resolve, status
- set-runtime, set-launcher
- doctor, cleanup, repair-marker, protect
- snapshot create, list, show, restore, remove, prune
- export, import
- remove, destroy, prune

### `release`

- list
- show
- install

### `runtime`

- add
- list
- show
- install
- update
- releases
- verify
- which
- remove

### `launcher`

- add
- list
- show
- remove

### `service`

- list
- status
- discover
- logs
- install
- start
- stop
- restart
- uninstall
- adopt-global
- restore-global

### `self`

- update

## Platform support

Current support:

- macOS
- Linux

Background services:

- macOS uses `launchd`
- Linux uses `systemd --user`

Windows service support is not implemented yet.

## Safety notes

`ocm` keeps safety checks around destructive actions.

Examples:

- protected environments are not removed unless forced
- environment roots are marker-checked before destructive cleanup
- preview-first behavior is used where it matters
- service install picks the next free port instead of colliding with an existing one

## Where to look next

- `ocm help`
- `ocm help start`
- `ocm help setup`
- `ocm help env`
- `ocm help release`
- `ocm help runtime`
- `ocm help launcher`
- `ocm help service`
