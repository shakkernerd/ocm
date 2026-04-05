# ocm

**Install, run, update, and manage OpenClaw — properly.**

OCM gives OpenClaw one coherent workflow across stable releases, local checkouts, background services, upgrades, snapshots, and ongoing maintenance.

OpenClaw is easy to start once. It gets messier when you want more than one setup, need stable and local development side by side, or want confidence about what is actually running. `ocm` fixes that.

Once an environment exists, `ocm` can be your normal OpenClaw entrypoint:

```bash
ocm @mira -- tui
ocm @mira -- status
ocm @mira -- onboard
```

## What ocm manages

`ocm` keeps the moving parts separate:

- **envs** — isolated OpenClaw environments
- **runtimes** — installed and pinned OpenClaw releases
- **launchers** — named command recipes for local-dev or custom runs
- **services** — background OpenClaw processes tied to one environment

That split is what makes stable releases, local development, upgrades, and service management fit together cleanly.

## Why people use it

Use `ocm` when you want:

- one clean OpenClaw environment per project, task, or instance
- one command path for OpenClaw itself through `ocm @<env> -- <command>`
- published OpenClaw releases installed locally and updated safely
- local checkout workflows that feel just as normal as released builds
- background services that are easy to inspect, update, restart, and remove
- snapshots, export/import, and safer cleanup

## Install

Install the latest release:

```bash
curl -fsSL https://raw.githubusercontent.com/shakkernerd/ocm/main/install.sh | bash
```

Install a specific release:

```bash
curl -fsSL https://raw.githubusercontent.com/shakkernerd/ocm/main/install.sh | bash -s -- --version v<ocm-version>
```

Update an existing install:

```bash
ocm self update
ocm self update --check
```

Install from source:

```bash
cargo install --path .
```

Inside this repo, use the development wrapper:

```bash
./bin/ocm help
```

Published OpenClaw release flows in `ocm` prefer host Node.js `22.14.0` or newer and `npm`.
On supported platforms, `ocm` can manage a private copy for official release installs when those tools are missing.
Interactive release setup can also offer to install `git` for repo-aware coding workflows when it is missing.
Local checkout flows keep using whatever command and toolchain you choose.

## Start quickly

If you want the guided path:

```bash
ocm setup
```

If you already know what you want:

```bash
ocm start
```

`setup` walks you through the choices. `start` creates or reuses an environment, installs the latest stable OpenClaw release by default, and keeps it running in the background. If you do not pass a name, `ocm` generates one for you.

## Common paths

### Use the latest stable release

```bash
ocm start mira
ocm @mira -- onboard
ocm @mira -- tui
```

This is the shortest path for most people.

### Update OpenClaw later

```bash
ocm upgrade mira
ocm upgrade --all
```

### Use a local checkout or dev build

```bash
ocm start luna --command 'pnpm openclaw' --cwd /path/to/openclaw --no-service
ocm @luna -- tui
```

Use `--no-service` when you want a quick local-dev setup without a background process. If you are already inside an OpenClaw checkout, `ocm setup` can detect that and suggest the local-command path automatically.

### Try beta or pin a specific release

```bash
ocm start rowan --channel beta
ocm start ember --version 2026.3.24
```

### Apply an optional project manifest

```bash
ocm up --dry-run
ocm up
ocm sync --dry-run
```

If a folder has an optional `ocm.yaml`, `up` can create or apply the env, binding, and service changes it declares, and `sync` can reconcile an env that already exists. Existing env applies are snapshotted first and rolled back if a later reconcile step fails. This is project mode, not a requirement for normal `setup` or `start` usage.

### Inspect an existing plain OpenClaw home before migrating it

```bash
ocm migrate inspect
ocm migrate inspect /path/to/.openclaw
ocm migrate plan --name mira
ocm migrate import --name mira
ocm migrate import --name mira --manifest ./ocm.yaml
```

`inspect` and `plan` are read-only. They show the plain OpenClaw home OCM would inspect and the env target it would use before any import work happens.

`migrate import` creates the managed env from a plain OpenClaw home, preserves durable config and agent auth, and clears copied runtime residue like sessions and logs. Add `--manifest <path>` if you also want OCM to write a minimal `ocm.yaml` after the import.

### Keep background services visible

```bash
ocm service list
ocm service status mira
ocm service logs mira --tail 50
```

## Why not just run OpenClaw directly?

Running OpenClaw directly is fine for the simplest case.

Use `ocm` when you want:

- more than one environment
- clean runtime and launcher separation
- stable and local-dev setups side by side
- inspectable background services
- safer upgrades, snapshots, and repair flows

Manual setup works. `ocm` is what makes it feel organized.

## Learn more

For the full guide, including scenarios and command details, see [docs/USAGE.md](docs/USAGE.md).

You can also use:

```bash
ocm help
ocm help start
ocm help setup
ocm help env
ocm help release
ocm help runtime
ocm help launcher
ocm help service
```

## Platform support

Current support:

- macOS
- Linux

Background services:

- macOS uses `launchd`
- Linux uses `systemd --user`

Windows service support is not implemented yet.

## License

See [LICENSE](LICENSE).
