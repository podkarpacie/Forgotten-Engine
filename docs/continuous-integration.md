# Continuous Integration

The repository includes one deterministic Rust quality workflow at
`.github/workflows/rust.yml`. It runs on pushes to `main` and on pull requests. The workflow
does not publish artifacts, upload releases, access credentials, or start a game server.

| Gate | Purpose |
|---|---|
| `cargo test --workspace` | Verifies unit, integration, and documentation tests in the workspace. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Treats Rust and Clippy warnings as failures. |
| `cargo fmt --all -- --check` | Rejects formatting drift. |
| Capability matrix JSON parse | Ensures the machine-readable compatibility matrix remains valid JSON. |
| `git diff --check HEAD^` | Rejects whitespace errors introduced by the change. |

The workflow intentionally does not claim complete protocol interoperability, platform release
validation, real-client compatibility, performance, or production readiness. Those need their own
evidence and release gates.
