# ocm

`ocm` is OpenClaw Manager.

It helps you run OpenClaw in separate environments, switch between them safely, install published releases, use local checkouts, and keep background services running without state collisions.

## What it does

- create isolated OpenClaw environments
- install and manage published OpenClaw releases
- run a local checkout or custom command through named launchers
- start and inspect background OpenClaw services per environment
- snapshot, export, import, clean up, and remove environments safely

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

## Quick start

If you want the easiest first run:

```bash
ocm setup
```

If you already know what you want, use the fast path:

```bash
ocm start
```

That creates or reuses an environment, prepares OpenClaw, and can start onboarding for you.

If you want the full guide, including scenarios and command details, see [docs/USAGE.md](docs/USAGE.md).

## The main ideas

- `env`: one isolated OpenClaw environment
- `release`: one published OpenClaw release you can inspect before installing
- `runtime`: one local OpenClaw install managed by `ocm`
- `launcher`: one named command for running OpenClaw, often used for local checkouts
- `service`: one background OpenClaw process tied to one environment

Most people should think about environments first. The rest exists to tell an environment what to run and whether it should stay running in the background.

## Fastest paths

### 1. Start with the latest stable release

```bash
ocm start
```

Or choose the environment name yourself:

```bash
ocm start mybot
```

### 2. Start with a beta release

```bash
ocm start mybot --channel beta
```

### 3. Start with a specific published release

```bash
ocm start mybot --version 2026.3.24
```

### 4. Start from a local checkout

```bash
ocm start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw
```

If you are already inside an OpenClaw checkout, `ocm setup` can detect that and suggest the local-command path automatically.

## Running commands

Activate an environment in your shell:

```bash
eval "$(ocm env use mybot)"
```

Run OpenClaw inside the active environment:

```bash
ocm -- status
ocm -- onboard
```

Run against a named environment without activating it first:

```bash
ocm @mybot -- status
ocm @mybot -- onboard
```

Run any command inside an environment:

```bash
ocm env exec mybot -- sh -lc 'echo "$OPENCLAW_HOME"'
```

## Background services

Install a persistent service for an environment:

```bash
ocm service install mybot
```

Inspect it:

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

`service list` is the quick overview. `service status <env>` is the detailed view.

## Working with releases and runtimes

List published releases:

```bash
ocm release list
ocm release list --channel stable
ocm release show --channel stable
```

Install one published release as a local runtime:

```bash
ocm release install --channel stable
ocm release install --version 2026.3.24
```

Inspect local runtimes:

```bash
ocm runtime list
ocm runtime show stable
ocm runtime verify stable
```

Update a tracked runtime:

```bash
ocm runtime update stable
ocm runtime update --all
```

## Using local commands and launchers

Register a launcher:

```bash
ocm launcher add dev --command 'pnpm openclaw' --cwd /path/to/openclaw
```

Create an environment that uses it:

```bash
ocm env create hacking --launcher dev
```

Inspect launchers:

```bash
ocm launcher list
ocm launcher show dev
```

## Environment lifecycle

Clone an environment:

```bash
ocm env clone mybot mybot-copy
```

Create and restore snapshots:

```bash
ocm env snapshot create mybot --label before-upgrade
ocm env snapshot list mybot
ocm env snapshot restore mybot <snapshot>
```

Export and import environments:

```bash
ocm env export mybot
ocm env import ./mybot.tar --name restored-mybot
```

Inspect and repair:

```bash
ocm env status mybot
ocm env doctor mybot
ocm env cleanup mybot --yes
ocm env repair-marker mybot
```

Remove an environment:

```bash
ocm env remove mybot
```

Remove the environment, its OCM-managed service, and its snapshots:

```bash
ocm env destroy mybot
ocm env destroy mybot --yes
```

## Output modes

Default terminal output is meant for people.

Use:

- `--json` for structured output
- `--raw` for plain script-friendly output
- `--color auto|always|never` for explicit color control

Examples:

```bash
ocm service list --json
ocm env status mybot --raw
ocm --color always runtime list
```

## Help

Start here:

```bash
ocm help
```

Then go deeper:

```bash
ocm help start
ocm help setup
ocm help env
ocm help release
ocm help runtime
ocm help service
```

## Platform support

Current support:

- macOS
- Linux

Background service support:

- macOS: `launchd`
- Linux: `systemd --user`

Not supported yet:

- Windows service support

## Safety

`ocm` keeps safety checks around destructive actions:

- protected environments are not removed unless forced
- environment roots are marker-checked before destructive cleanup
- prune operations preview first
- service install chooses the next free port instead of colliding with an existing one

## Typical workflow

For most users:

1. `ocm setup`
2. let it create an environment
3. finish OpenClaw onboarding
4. use `ocm service install <env>` if that environment should stay running
5. use `ocm @<env> -- status` and `ocm service status <env>` as your main checks

For local development:

1. `ocm start hacking --command 'pnpm openclaw' --cwd /path/to/openclaw`
2. `ocm @hacking -- onboard`
3. `ocm service install hacking` if you want it running in the background

## License

See [LICENSE](LICENSE).
