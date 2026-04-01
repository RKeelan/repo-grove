# AGENTS.md

A Rust CLI tool for managing a collection of Git repositories.

## Commands

- `cargo build` — build the project
- `cargo test` — run tests
- `cargo fmt --all -- --check` — check formatting
- `cargo clippy --all-targets -- -D warnings` — lint

## Dependency Policy

All dependencies are pinned to exact versions (e.g., `serde = "1.0.210"`). Do not use version ranges (`^`, `~`, `>=`). Dependabot handles upgrades.
