# Test-only boundary

`FakeClock` and namespaced `SequenceIds` provide deterministic in-memory inputs.
`jojo-domain` uses this package only as a dev-dependency; the helper does not link it.
These generators are not restart-safe production IDs. Thread ordering is not promised;
tests assert uniqueness and deterministic sets, not incidental thread interleaving.

`ProviderKind` labels in tests represent fake identities, not executable adapters.
No Start/LaunchSpec/event protocol/fake-provider runner is implemented here. Those
belong to P1-08, and subprocess fixtures to P1-09 after their prerequisites. Do not
name or discover fixture programs as `claude` or `codex` on PATH.
