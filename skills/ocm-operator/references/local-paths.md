# Local Paths And Defaults

These defaults match Shakker's current OpenClaw/OCM workspace. Verify with
`pwd`, `git status --short --branch`, and `ocm env list` before relying on
them.

## Repos

OCM repo:

```text
/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/ocm
```

OpenClaw release/build test repo:

```text
/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/test-build
```

OpenClaw dev/plugin test repo:

```text
/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/test-install
```

Active OpenClaw repo to avoid unless the user explicitly permits it:

```text
/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/openclaw
```

## Long-Lived Envs

Treat these as real user state unless the user explicitly says otherwise:

```text
Shaks
Violet
```

Use `Violet` as the source for existing-user clone tests:

```sh
ocm env clone Violet <test-env>
```

## Common Test Names

Use descriptive, disposable names:

```text
Violet-local-release-test
fresh-local-release-test
fresh-local-onboard-test
plugins-dev-test
```

Destroy them when done:

```sh
ocm env destroy Violet-local-release-test --yes
ocm env destroy fresh-local-release-test --yes
ocm env destroy fresh-local-onboard-test --yes
ocm env destroy plugins-dev-test --yes
```

## Local Release Runtime Pattern

```sh
set -euo pipefail

source_repo=/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/test-build
run_id="$(date -u +%Y%m%dT%H%M%SZ)-$$"
run_root="/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/release-validation/${run_id}"
worktree="${run_root}/openclaw"
runtime="openclaw-${run_id}"

test "$(git -C "$source_repo" rev-parse --show-toplevel)" = "$source_repo"
git -C "$source_repo" fetch origin main --prune
openclaw_sha="$(git -C "$source_repo" rev-parse origin/main)"
mkdir -p "$run_root"
git -C "$source_repo" worktree add --detach "$worktree" "$openclaw_sha"
test "$(git -C "$worktree" rev-parse HEAD)" = "$openclaw_sha"
test -z "$(git -C "$worktree" status --porcelain)"

cd "$worktree"
pnpm install
pnpm check
pnpm build

cd /Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/ocm
ocm runtime build-local "$runtime" --repo "$worktree" --force
ocm runtime verify "$runtime"
ocm runtime show "$runtime"
node "$HOME/.ocm/runtimes/$runtime/files/node_modules/openclaw/openclaw.mjs" --version

test "$(git -C "$worktree" rev-parse HEAD)" = "$openclaw_sha"
```

Keep `run_id`, `run_root`, `worktree`, `runtime`, env names, and the report
together. Cleanup must target only those names and must inspect worktree status
before removal.

## Release Validation Cheatsheet

If present, this local doc has a broader workflow checklist:

```text
/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/ocm/docs/TESTING_WORKFLOW_CHEATSHEET.md
```

It is a local ignored doc for this checkout.
