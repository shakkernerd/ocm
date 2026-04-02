# ocm

`ocm` is OpenClaw Manager.

It gives you one clean way to install, run, update, and keep OpenClaw isolated.

Once an environment exists, `ocm` can be your normal OpenClaw entrypoint:

```bash
ocm @mybot -- tui
ocm @mybot -- status
ocm @mybot -- onboard
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

Published OpenClaw release flows in `ocm` need Node.js `22.14.0` or newer and `npm` on `PATH`.
Local checkout flows keep using whatever command and toolchain you choose.
Run `ocm doctor host` on a new machine if you want one quick readiness check before using release flows.

## Start quickly

If you want the guided path:

```bash
ocm doctor host
ocm setup
```

If you already know what you want:

```bash
ocm doctor host
ocm start
```

`setup` walks you through the choices. `start` creates or reuses an environment, installs the latest stable OpenClaw release by default, and keeps it running in the background. If you do not pass a name, `ocm` generates one for you.

## Common paths

### Use the latest stable release

```bash
ocm start mybot
ocm @mybot -- onboard
ocm @mybot -- tui
```

This is the shortest path for most people.

### Update OpenClaw later

```bash
ocm upgrade mybot
ocm upgrade --all
```

### Use a local checkout or dev build

```bash
ocm start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw --no-service
ocm @hacking -- tui
```

Use `--no-service` when you want a quick local-dev setup without a background process. If you are already inside an OpenClaw checkout, `ocm setup` can detect that and suggest the local-command path automatically.

### Try beta or pin a specific release

```bash
ocm start beta-bot --channel beta
ocm start pinned-bot --version 2026.3.24
```

### Keep background services visible

```bash
ocm service list
ocm service status mybot
ocm service logs mybot --tail 50
```

## Learn more

For the full guide, including scenarios and command details, see [docs/USAGE.md](docs/USAGE.md).

You can also use:

```bash
ocm help
ocm help doctor
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
