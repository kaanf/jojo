# Native macOS shell (P1-14)

Dependency-free Swift package, macOS 14+. No remote packages or package resolution.

- `JojoAppBoundary` — the original P1-01 scene protocol (unchanged).
- `JojoCore` — Foundation-only P1-13 client: strict JSON/envelope codec, the helper
  transport (launches exactly `jojo-helper --serve`; never signals a process), request and
  receipt correlation, the quit coordinator, review/run/recovery projections and the U02
  window policy. No SQLite, networking or file administration.
- `JojoApp` — SwiftUI content in AppKit-owned windows. The application delegate owns
  termination: Cmd-Q (SwiftUI replacement of the Quit command), the quit Apple event and
  last-primary-window close all enter one coordinator. Quitting defers with
  `terminateLater` and replies only after a valid ShutdownReady receipt **and** an observed
  successful helper exit. Auxiliary windows close independently.
- `JojoCoreTests` — T14 model tests against a barrier-driven fake helper, plus tests
  against the real helper when `JOJO_HELPER_BIN` is set.

Run for development (the helper must be built first):

    cargo build -p jojo-helper
    swift run --package-path macos/JojoApp JojoApp --helper target/debug/jojo-helper

Without `--data-root/--worktree-root` the helper uses the accepted private Application
Support roots. `--trace FILE` and `--ui-script STEPS` exist for runtime GUI verification.

Full gate, including runtime GUI scenarios on WindowServer: `python3 scripts/verify_p1_14.py`.
