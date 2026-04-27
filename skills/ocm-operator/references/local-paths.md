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
cd /Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/test-build
git switch main
git pull origin main
pnpm install
pnpm check
pnpm build

cd /Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/ocm
ocm runtime build-local test-build-1 --repo /Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/temp/test-build --force
ocm runtime verify test-build-1
ocm runtime show test-build-1
node "$HOME/.ocm/runtimes/test-build-1/files/node_modules/openclaw/openclaw.mjs" --version
```

## Release Validation Cheatsheet

If present, this local doc has a broader workflow checklist:

```text
/Users/shakker/WorkSpace/ShakkerNerd/OpenSource/OpenClaw/ocm/docs/TESTING_WORKFLOW_CHEATSHEET.md
```

It is a local ignored doc for this checkout.
