# Changelog

All notable changes to OCM are documented here.

## Unreleased

### Fixed

- Keep automatically assigned gateway ports stable across create, show, list, and status output.
- Give cloned and imported environments independent service state and collision-free gateway ports.
- Serialize environment registry transactions so concurrent lifecycle commands preserve every entry and allocate distinct ports.
- Preserve environment roots and recovery snapshots when dev worktree cleanup fails during destroy.
