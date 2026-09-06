# Headless runtime integration tests

Run from the repository root:

```sh
cargo test --manifest-path tests/runtime/Cargo.toml --target-dir overlay/target
```

This standalone test crate compiles the actual shell controller, journal store,
poller, probe, rune queue, and settings modules by relative path. Its minimal App and Tauri handle
stubs replace window emission only; client-import wrappers are inert test stubs.
The poller future is checked for Send but is never run. No test contacts the
League client, launches a window, or writes to a game account.

Tests cover immediate preference/pin changes, target completion, stale or
unknown identity, match/champion resets, recap feedback, corruption preservation,
coalesced journal writes to test-owned temporary directories, and diagnostic
champion/role selection without calling the probe, and import identity preserved
while waiting on a rune-write mutex. They do not
replace the production Windows/Tauri build or verify IPC macro registration.
