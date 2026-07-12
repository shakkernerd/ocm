# Changelog

All notable changes to OCM are documented here.

## Unreleased

### Fixed

- Add atomic state-token guards for automation that previews and destroys environments.
- Keep automatically assigned gateway ports stable across create, show, list, and status output.
- Give cloned and imported environments independent service state and collision-free gateway ports.
- Serialize environment registry transactions so concurrent lifecycle commands preserve every entry and allocate distinct ports.
- Preserve environment roots and recovery snapshots when dev worktree cleanup fails during destroy.
- Keep auto-assigned ports stable without overriding a later gateway port written by OpenClaw configuration.
- Reject non-UTF-8 process input cleanly, require version flags to stand alone, and terminate quietly when a pipeline closes its output.
- Prevent Fish activation injection through crafted paths and clear stale OpenClaw controls when switching environments.
- Keep tables within narrow terminal widths by truncating headers or falling back to compact rows.
- Verify self-update release digests and staged binary versions before replacement, compare releases with SemVer precedence, clean temporary artifacts on failure, and reject Linux ARM64 until release assets are published.
- Reject launcher deletion while environments still depend on it, and route shell-expanding launcher recipes through a shell instead of lossy direct execution.
- Accept the standard human-output flags for `runtime releases` and document `runtime build-local` in the command map.
- Keep plain-home migration path rewrites scoped to the imported OpenClaw state and reject overlapping source and target roots before mutation.
- Fail migration planning on corrupt environment registries and only auto-bind executable OpenClaw commands from `PATH`.
- Report launcher and environment rollback failures alongside the original migration error instead of hiding partial cleanup.
- Make release preparation rollback-safe, verify signed annotated tags on every resume path, and publish branch and tag refs atomically.
- Build locked release artifacts only from a GitHub-verified tag matching the package version, require the complete supported target matrix before publication, publish a versioned checksum-verifying installer, pin CI actions, and test the declared Rust 1.88 minimum.
- Preserve uncommitted dev worktrees and reject unrelated checkout collisions by requiring exact Git worktree registration before reuse or removal.
- Make single and merged log following rotation-safe and byte-safe, preserving split or invalid UTF-8, multiline record ordering, and boundaries around unterminated records.
