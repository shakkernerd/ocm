# ocm

`ocm` is OpenClaw Manager.

It gives you a clean way to run OpenClaw in separate environments, use published releases or local checkouts, and keep each setup isolated and easy to manage.

Once an environment exists, `ocm` can be your normal OpenClaw entrypoint:

```bash
ocm @mybot -- status
ocm @mybot -- onboard
ocm @mybot -- gateway run
```

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

## The easiest start

If you want the guided path:

```bash
ocm setup
```

If you already know what you want:

```bash
ocm start
```

`setup` and `start` keep OpenClaw running in the background by default. If you do not pass a name, `ocm` generates one for you.

## Common paths

### Simple: start one OpenClaw environment

```bash
ocm start mybot
ocm @mybot -- onboard
ocm @mybot -- status
```

This is the shortest path for most people. `ocm` installs the latest stable OpenClaw release, creates the environment, and keeps it running in the background.

From there, use `ocm` for OpenClaw itself:

```bash
ocm @mybot -- status
ocm @mybot -- onboard
ocm @mybot -- gateway run
```

### Update OpenClaw later

```bash
ocm upgrade mybot
ocm upgrade --all
```

### Use a local checkout

```bash
ocm start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw --no-service
ocm @hacking -- onboard
```

Use `--no-service` when you want a quick local-dev setup without a background process. If you are already inside an OpenClaw checkout, `ocm setup` can detect that and suggest the local-command path automatically.

### Try beta or pin a specific release

```bash
ocm start preview --channel beta
ocm start pinned --version 2026.3.24
```

### Check and manage running services

```bash
ocm service list
ocm service status mybot
ocm service logs mybot --tail 50
```

## What makes it useful

`ocm` is built for both everyday use and heavier workflows.

It can:

- keep stable, beta, pinned, and local-dev OpenClaw setups side by side
- manage published releases as local runtimes
- run local checkouts through launchers
- make `ocm @<env> -- <command>` the normal way to run OpenClaw
- keep background services tied to the right environment
- snapshot, export, import, repair, and destroy environments safely

## A few commands worth knowing

```bash
ocm help
ocm upgrade mybot
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
