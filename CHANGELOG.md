# Changelog

All notable changes to OCM are documented here.

## Unreleased

### Fixed

- Keep automatically assigned gateway ports stable across create, show, list, and status output.
- Give cloned and imported environments independent service state and collision-free gateway ports.
- Serialize environment registry transactions so concurrent lifecycle commands preserve every entry and allocate distinct ports.
- Preserve environment roots and recovery snapshots when dev worktree cleanup fails during destroy.
- Keep auto-assigned ports stable without overriding a later gateway port written by OpenClaw configuration.
- Reject non-UTF-8 process input cleanly, require version flags to stand alone, and terminate quietly when a pipeline closes its output.
- Prevent Fish activation injection through crafted paths and clear stale OpenClaw controls when switching environments.
- Keep tables within narrow terminal widths by truncating headers or falling back to compact rows.
