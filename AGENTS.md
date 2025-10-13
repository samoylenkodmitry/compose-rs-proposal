# Agent Notes for compose-rs-proposal

- This repo is a Rust workspace; run `cargo fmt` and `cargo clippy --all-targets --all-features` before committing major code changes.
- Most code lives in the `compose-*` crates. Add nested `AGENTS.md` files for crate-specific guidance if necessary.
- Use `cargo test -p <crate_name>` for targeted checks or `cargo test` for the full workspace.
