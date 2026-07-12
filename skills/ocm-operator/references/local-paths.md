# Local Paths And Defaults

Configure these placeholders for the current OpenClaw/OCM workspace. Verify
with `pwd`, `git status --short --branch`, and `ocm env list` before relying
on them.

## Repos

OCM repo:

```text
/path/to/ocm
```

OpenClaw release/build test repo:

```text
/path/to/openclaw-source-cache
```

OpenClaw dev/plugin test repo:

```text
/path/to/openclaw-plugin-test
```

Active OpenClaw repo to avoid unless the user explicitly permits it:

```text
/path/to/active-openclaw
```

## Long-Lived Envs

Treat these as real user state unless the user explicitly says otherwise:

```text
<existing-env>
<second-existing-env>
```

Use `<existing-env>` as the source for existing-user clone tests:

```sh
ocm env clone <existing-env> <test-env>
```

## Common Test Names

Use descriptive, disposable names:

```text
existing-env-local-release-test
fresh-local-release-test
fresh-local-onboard-test
plugins-dev-test
```

Destroy them when done:

```sh
ocm env destroy existing-env-local-release-test --yes
ocm env destroy fresh-local-release-test --yes
ocm env destroy fresh-local-onboard-test --yes
ocm env destroy plugins-dev-test --yes
```

## Local Release Runtime Pattern

```sh
set -euo pipefail

source_repo=/path/to/openclaw-source-cache
ocm_repo=/path/to/ocm
OCM_BIN="${ocm_repo}/target/debug/ocm"
validation_root=/path/to/release-validation
mkdir -p "$validation_root"
run_root="$(mktemp -d "${validation_root}/run-XXXXXXXXXX")"
run_id="${run_root##*/}"
repo_store="${run_root}/repo"
worktree="${run_root}/openclaw"
runtime="openclaw-${run_id}"
export OCM_HOME="${run_root}/ocm-home"

test "$(git -C "$source_repo" rev-parse --show-toplevel)" = "$source_repo"
test -x "$OCM_BIN"
source_remote="$(git -C "$source_repo" remote get-url origin)"
mkdir -p "$OCM_HOME"
git clone --no-checkout --single-branch --branch main \
  --reference-if-able "$source_repo" --dissociate \
  "$source_remote" "$repo_store"
openclaw_sha="$(git -C "$repo_store" rev-parse origin/main)"
git -C "$repo_store" worktree add --detach "$worktree" "$openclaw_sha"
test "$(git -C "$worktree" rev-parse HEAD)" = "$openclaw_sha"
test -z "$(git -C "$worktree" status --porcelain)"

cd "$worktree"
pnpm install --frozen-lockfile
pnpm check
pnpm build
test -z "$(git -C "$worktree" status --porcelain)"

cd "$ocm_repo"
"$OCM_BIN" runtime build-local "$runtime" --repo "$worktree" --force
"$OCM_BIN" runtime verify "$runtime"
"$OCM_BIN" runtime show "$runtime"
runtime_bin="$("$OCM_BIN" runtime which "$runtime" --raw)"
"$runtime_bin" --version

test "$(git -C "$worktree" rev-parse HEAD)" = "$openclaw_sha"
test -z "$(git -C "$worktree" status --porcelain)"
```

Keep `run_id`, `run_root`, `repo_store`, `worktree`, `runtime`, env names, and
the report together. Cleanup must target only those names and must inspect
worktree status before removal. Destroy dependent envs before running
`"$OCM_BIN" runtime remove "$runtime"`.

## Release Validation Cheatsheet

If present, this local doc has a broader workflow checklist:

```text
<ocm-repo>/docs/TESTING_WORKFLOW_CHEATSHEET.md
```

It is a local ignored doc for this checkout.
