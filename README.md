# repo-grove

[![CI](https://github.com/RKeelan/repo-grove/actions/workflows/ci.yml/badge.svg)](https://github.com/RKeelan/repo-grove/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/repo-grove.svg)](https://crates.io/crates/repo-grove)

A Rust CLI tool for managing a collection of Git repositories.

## Commands

- `grove index` — build or refresh the repository index (discovers repos from GitHub, maps to local paths)
- `grove update` — fetch, pull, and report readiness for local checkouts
- `grove prune` — prune merged branches
- `grove ls prs|ci|issues` — list open Dependabot PRs, failing CI runs, or open issues
- `grove dependabot merge` — auto-merge open Dependabot PRs
- `grove api <endpoint>` — run a `gh api` call against indexed repos

## Dev Commands

- `cargo build` — build the project
- `cargo test` — run tests
- `cargo fmt` — format code
- `cargo clippy --all-targets -- -D warnings` — lint
- `cargo install --path .` — install the `grove` binary
- `cargo publish` — publish to crates.io
- `gh release create vX.Y.Z --generate-notes` — tag and create a GitHub release
