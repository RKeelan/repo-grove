# repo-grove

[![CI](https://github.com/RKeelan/repo-grove/actions/workflows/ci.yml/badge.svg)](https://github.com/RKeelan/repo-grove/actions/workflows/ci.yml)

A Rust CLI tool for managing a collection of Git repositories.

## Commands

- `grove index` — build or refresh the repository index (discovers repos from GitHub, maps to local paths)
- `grove update` — fetch, pull, and report readiness for local checkouts

## Dev Commands

- `cargo build` — build the project
- `cargo test` — run tests
- `cargo fmt` — format code
- `cargo clippy --all-targets -- -D warnings` — lint
