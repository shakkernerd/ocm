# ocm

**Install, run, update, and manage OpenClaw — properly.**

OCM gives OpenClaw one coherent workflow across stable releases, local checkouts, supervised env gateways, upgrades, snapshots, and ongoing maintenance.

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
- one OCM background service that can supervise env gateways cleanly
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

If you are developing OpenClaw itself, use the dev path:

```bash
ocm dev shaks
ocm dev shaks --root /tmp/shaks
ocm dev shaks --watch
ocm dev shaks --onboard
```

`dev` creates or reuses an isolated env, provisions an OpenClaw worktree under the repo's own `.worktrees/`, bootstraps the minimum local config so the gateway can run immediately, and then starts the gateway in the foreground. `--root` lets you place that env anywhere. `--watch` keeps a source-run gateway rebuilding in place. `--onboard` runs local onboarding first and then starts the dev gateway.

If you already have a plain `~/.openclaw` home you care about, use `ocm migrate <env>` instead of starting fresh. `setup` and `start` now point that out when they detect an existing plain OpenClaw home.

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
ocm dev luna
ocm dev luna --root ~/scratch/luna
ocm dev luna --watch
```

Use `ocm dev` when you want an isolated source-run checkout with its own env root and gateway port. If you are already inside an OpenClaw checkout, `ocm setup` can detect that and suggest a local path automatically.

### Try beta or pin a specific release

```bash
ocm start rowan --channel beta
ocm start ember --version 2026.3.24
```

### Inspect an existing plain OpenClaw home before migrating it

```bash
ocm migrate mira
ocm migrate mira /path/to/.openclaw
ocm adopt inspect
ocm adopt plan --name mira
```

`migrate` is the simple front door for existing OpenClaw users. It imports a plain OpenClaw home into a managed env in one step.

`migrate` preserves config, auth, sessions, logs, and other durable user state, rewrites env-scoped paths for the new managed root, and clears only live runtime residue like locks, pid files, and sockets. If `openclaw` is already available on `PATH`, it also binds the imported env to an env-local migrated launcher so you can keep using it through OCM immediately.

`adopt inspect` and `adopt plan` are the explicit read-only preview tools. Use them when you want to inspect the plain OpenClaw home OCM would read or preview the target env/root before importing.

### Keep supervised envs visible

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
- inspectable supervised env gateways
- safer upgrades, snapshots, and repair flows

Manual setup works. `ocm` is what makes it feel organized.

## Learn more

For the full guide, including scenarios and command details, see [docs/USAGE.md](docs/USAGE.md).

You can also use:

```bash
ocm help
ocm help dev
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
