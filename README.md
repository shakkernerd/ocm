# ocm

OpenClaw Manager.

The first goal is a disposable OpenClaw development workflow that never touches your main `~/.openclaw` state. The design centers on `OPENCLAW_HOME`, because that moves OpenClaw's home-derived paths together and makes teardown predictable.

If you are continuing work in a fresh session, read `AGENTS.md` and `docs/HANDOFF.md` first.

## What it does now

- Creates isolated OpenClaw environments with their own `OPENCLAW_HOME`
- Prints shell activation snippets for `eval "$(ocm env use <name>)"`
- Runs arbitrary commands inside an env with the correct OpenClaw variables injected
- Binds envs to named "versions" that point at a specific OpenClaw launcher command and checkout
- Removes envs safely and supports previewable or applied pruning

## Project layout

- `src/main.rs`: CLI entrypoint
- `src/cli.rs`: command routing and process execution
- `src/store.rs`: metadata and filesystem operations
- `src/paths.rs`: path derivation and naming rules
- `src/shell.rs`: shell activation rendering and env injection
- `src/types.rs`: shared structs
- `bin/ocm`: dev convenience wrapper

## Build and run

Build:

```bash
cargo build
./target/debug/ocm help
```

Or use the wrapper:

```bash
./bin/ocm help
```

## Usage

Register an OpenClaw launcher as a version:

```bash
./bin/ocm version add stable --command openclaw
```

Create a disposable env tied to that version:

```bash
./bin/ocm env create refactor-a --version stable --port 19789
```

Activate it in your shell:

```bash
eval "$(./bin/ocm env use refactor-a)"
```

Run OpenClaw inside the env through the registered launcher:

```bash
./bin/ocm env run refactor-a -- onboard
./bin/ocm env run refactor-a -- gateway run --port 19789
```

Run any arbitrary command inside the env:

```bash
./bin/ocm env exec refactor-a -- openclaw status
```

Preview pruning of old disposable envs:

```bash
./bin/ocm env prune --older-than 7
```

Apply pruning:

```bash
./bin/ocm env prune --older-than 7 --yes
```

Remove a specific env:

```bash
./bin/ocm env remove refactor-a
```

## Notes

- Env roots default to `~/.ocm/envs/<name>`.
- Relative `OCM_HOME` values are normalized against the current working directory before store paths are derived.
- Each env root owns its own `.openclaw/` tree.
- `ocm env use` explicitly unsets `OPENCLAW_PROFILE` to avoid profile and state-dir collisions.
- Protected envs are skipped by prune and require `--force` for removal.
- Versions are launcher definitions for now. This keeps the tool flexible enough to target:
  - a globally installed `openclaw`
  - a packaged binary

## Next useful steps

- Automated tests for metadata compatibility and CLI parsing.
- Shell helper integration so `ocm use <name>` works like `nvm use`
- Real version install flows, not just launcher registration
- Snapshot and export/import for env roots
- Port allocation helpers
- Status and cleanup commands for running gateways
