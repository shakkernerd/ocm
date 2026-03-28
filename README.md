# ocm

`ocm` is OpenClaw Manager.

It gives you a clean way to run OpenClaw in separate environments, use published releases or local checkouts, and keep background services running without state collisions.

## Why people use it

Use `ocm` when you want:

- one clean OpenClaw environment per project, task, or instance
- published OpenClaw releases installed locally and updated safely
- local checkout workflows that feel just as normal as released builds
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

## The easiest start

If you want the guided path:

```bash
ocm setup
```

If you already know what you want:

```bash
ocm start
```

If you do not pass a name, `ocm` generates one for you.

## Common paths

### Simple: start one OpenClaw environment

```bash
ocm start mybot
ocm @mybot -- onboard
ocm @mybot -- status
```

This is the shortest path for most people.

### Keep it running in the background

```bash
ocm service install mybot
ocm service status mybot
ocm service logs mybot --tail 50
```

Use this when the environment should keep running after you close the terminal.

### Use a local checkout

```bash
ocm start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw
ocm @hacking -- onboard
```

If you are already inside an OpenClaw checkout, `ocm setup` can detect that and suggest the local-command path automatically.

### Try beta or pin a specific release

```bash
ocm start preview --channel beta
ocm start pinned --version 2026.3.24
```

## What makes it useful

`ocm` is built for both everyday use and heavier workflows.

It can:

- keep stable, beta, pinned, and local-dev OpenClaw setups side by side
- manage published releases as local runtimes
- run local checkouts through launchers
- inspect and manage background services per environment
- snapshot, export, import, repair, and destroy environments safely

## A few commands worth knowing

```bash
ocm help
ocm release list
ocm runtime list
ocm service list
ocm env status mybot
ocm env snapshot create mybot --label before-upgrade
ocm env destroy mybot
```

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
