# ocm

OpenClaw Manager.

`ocm` is the local control plane for OpenClaw environments, runtimes, launchers, and persistent services.

It exists for one practical reason: running multiple OpenClaw setups on one machine should feel normal, not fragile.

With `ocm` you can:

- create isolated OpenClaw environments
- bind an environment to a launcher or runtime
- activate or run inside that environment safely
- install and verify managed runtimes
- run persistent OpenClaw services per environment
- clone, snapshot, export, import, repair, and clean up environments

## Why use it

Bare OpenClaw is fine when you have one setup and one state directory.

It gets painful when you need:

- separate OpenClaw state per project, branch, or customer
- a stable dev launcher and a stable packaged runtime side by side
- persistent instances on different ports without collisions
- predictable cleanup and teardown
- clear service visibility instead of guessing what is running

`ocm` makes the environment the primary unit. Each env gets its own `OPENCLAW_HOME`, so state stays isolated and disposable.

## Product model

- `env`: an isolated OpenClaw environment
- `launcher`: a named command recipe for running OpenClaw
- `runtime`: a registered or installed OpenClaw binary/toolchain
- `service`: a persistent OpenClaw gateway bound to one env

## Install

### Release install

Use the install script:

```bash
curl -fsSL https://raw.githubusercontent.com/shakkernerd/ocm/main/install.sh | bash
```

Install a specific release or prefix:

```bash
curl -fsSL https://raw.githubusercontent.com/shakkernerd/ocm/main/install.sh | bash -s -- --version v0.1.0
curl -fsSL https://raw.githubusercontent.com/shakkernerd/ocm/main/install.sh | bash -s -- --prefix ~/.local
```

### From source

```bash
cargo install --path .
```

### Development wrapper

Inside the repo:

```bash
./bin/ocm help
```

The wrapper is for local development and keeps examples pointed at the repo-local command.

## Quickstart

Register a launcher:

```bash
ocm launcher add stable --command openclaw
```

Create an environment:

```bash
ocm env create demo --launcher stable
```

Activate it in your shell:

```bash
eval "$(ocm env use demo)"
```

Run OpenClaw in the active env:

```bash
ocm -- status
ocm -- onboard
```

Run against a named env without activating it first:

```bash
ocm @demo -- status
ocm @demo -- onboard
```

Install a persistent service for that env:

```bash
ocm service install demo
ocm service status demo
ocm service logs demo --tail 20
```

## Daily workflow

The launcher-first path is still the easiest way to get started.

1. Register one launcher for the OpenClaw command you normally use.
2. Create one env per project, task, or production instance.
3. Use `ocm env use <name>` for interactive work.
4. Use `ocm -- ...` or `ocm @<env> -- ...` for fast command execution.
5. Install `ocm service` for any env that should keep running.

Example:

```bash
ocm launcher add dev --command 'pnpm openclaw' --cwd /path/to/openclaw
ocm env create hacking --launcher dev
ocm @hacking -- onboard
ocm service install hacking
ocm service start hacking
ocm service status hacking
```

## Runtime management

`ocm` supports both registered binaries and installer-managed runtimes.

Register an existing binary:

```bash
ocm runtime add stable --path /path/to/openclaw
ocm env create prod --runtime stable
```

Install a managed runtime:

```bash
ocm runtime install nightly --url https://example.test/openclaw-nightly
ocm runtime verify nightly
ocm runtime which nightly
```

Inspect and update:

```bash
ocm runtime list
ocm runtime show nightly
ocm runtime update nightly
ocm runtime update --all
```

## Services

`ocm service` makes OpenClaw instances persistent and inspectable.

Core commands:

```bash
ocm service list
ocm service status demo
ocm service install demo
ocm service start demo
ocm service stop demo
ocm service restart demo
ocm service uninstall demo
ocm service logs demo --tail 50
ocm service discover
```

What `service list` tells you:

- which envs have OCM-managed services
- which port each env is using
- service-manager state
- actual OpenClaw reachability

What `service discover` tells you:

- OCM-managed services
- separate OpenClaw services outside OCM
- foreign/self-managed OpenClaw services discovered on the machine

## Environment lifecycle

`ocm` already includes the lifecycle tools needed to treat envs as real operational units:

```bash
ocm env clone demo demo-copy
ocm env snapshot create demo --label before-upgrade
ocm env snapshot list demo
ocm env snapshot restore demo <snapshot-id>
ocm env export demo
ocm env import ./demo.tar --name restored-demo
ocm env doctor demo
ocm env cleanup demo --yes
ocm env repair-marker demo
```

## Output modes

By default, `ocm` prefers human-friendly terminal output.

Use these when you need something else:

- `--json`: structured output
- `--raw`: plain script-friendly output
- `--color auto|always|never`: explicit color control

Examples:

```bash
ocm service list --json
ocm env status demo --raw
ocm --color always runtime list
```

## Platform support

Current support:

- macOS: envs, launchers, runtimes, lifecycle, shell helpers, and persistent services via `launchd`
- Linux: envs, launchers, runtimes, lifecycle, shell helpers, and persistent services via `systemd --user`

Current limitation:

- `service adopt-global` and `service restore-global` are currently for the legacy machine-wide OpenClaw service model and remain launchd-oriented
- Windows service support is not implemented yet

## Safety

`ocm` keeps safety rails on destructive operations:

- protected envs are not removed unless explicitly forced
- env roots are marker-checked before destructive cleanup
- prune is preview-first
- service install auto-assigns the next free gateway port instead of crashing into a conflict

## Help

```bash
ocm help
ocm help env
ocm help runtime
ocm help service
ocm help service install
```

## Development

```bash
cargo build
./target/debug/ocm help
env CARGO_HOME=/tmp/ocm-cargo-home CARGO_TARGET_DIR="$PWD/target" cargo test
```
