# ocm

`ocm` is OpenClaw Manager.

It helps you run OpenClaw in separate environments, switch between them safely, use published releases or local checkouts, and keep background services running without state collisions.

## Why use it

Use `ocm` when you want:

- one clean OpenClaw environment per project, task, or instance
- published OpenClaw releases installed locally and updated safely
- local checkout or custom command workflows without manual shell setup
- background services that are easy to inspect, start, stop, and remove
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

## Get started

If you want the easiest first run:

```bash
ocm setup
```

If you already know what you want:

```bash
ocm start
```

That creates or reuses an environment, prepares OpenClaw, and can start onboarding for you. If you do not pass a name, `ocm` generates one for you.

## Common paths

### Use the latest stable release

```bash
ocm start
```

Or choose the environment name yourself:

```bash
ocm start mybot
```

### Use the beta release

```bash
ocm start mybot --channel beta
```

### Pin to a specific published release

```bash
ocm start mybot --version 2026.3.24
```

### Use a local checkout

```bash
ocm start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw
```

If you are already inside an OpenClaw checkout, `ocm setup` can detect that and suggest the local-command path automatically.

## Daily use

Activate an environment in your shell:

```bash
eval "$(ocm env use mybot)"
```

Run OpenClaw in the active environment:

```bash
ocm -- status
ocm -- onboard
```

Run against a named environment without activating it first:

```bash
ocm @mybot -- status
ocm @mybot -- onboard
```

Run any other command inside an environment:

```bash
ocm env exec mybot -- sh -lc 'echo "$OPENCLAW_HOME"'
```

## Background services

Install a persistent service for an environment:

```bash
ocm service install mybot
```

Check it:

```bash
ocm service list
ocm service status mybot
ocm service logs mybot --tail 50
```

Stop or remove it:

```bash
ocm service stop mybot
ocm service uninstall mybot
```

## Published releases, runtimes, and local commands

`ocm` can work in three main ways:

- `release`: browse published OpenClaw releases
- `runtime`: use a local OpenClaw install managed by `ocm`
- `launcher`: use a local checkout or custom command

Examples:

```bash
ocm release list
ocm release install --channel stable
ocm runtime list
ocm runtime verify stable
ocm launcher add dev --command 'pnpm openclaw' --cwd /path/to/openclaw
```

## More than just setup

`ocm` also supports:

- environment status and health checks
- snapshots
- export and import
- cleanup and marker repair
- safe environment teardown with `env destroy`

If you want the full guide, including scenarios and command details, see [docs/USAGE.md](docs/USAGE.md).

## Help

Start here:

```bash
ocm help
```

Then go deeper:

```bash
ocm help setup
ocm help start
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
