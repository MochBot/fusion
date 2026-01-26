# Development Guide

This guide is for contributors working on the Fusion Rust/WASM codebase.

## Prerequisites

- Rust toolchain (stable)
- wasm-pack
- Headless Chrome (for WASM tests)

## Build

```bash
# Build all crates
cargo build --workspace

# Build WASM bindings
wasm-pack build crates/wasm --target web
```

## Tests

```bash
# Rust unit tests (125)
cargo test --workspace

# Parity matrix (56 scenarios)
cargo test --test parity_matrix

# WASM browser tests (14)
wasm-pack test --headless --chrome crates/wasm
```

Note: `cargo test` does NOT run WASM tests.

## Lint and Format

```bash
cargo clippy --workspace --all-targets
cargo fmt --all
```

## Repository Conventions

- Follow `README.md` and `ARCHITECTURE.md` for code style and conventions
- Avoid `.unwrap()` or `panic!` in production code
- Use dynamic ruleset configuration (avoid hardcoded presets)

## VCS Notes

I use Jujutsu (JJ) locally, but Git is the standard workflow for contributors.
Use Git commands below. If you prefer JJ, bookmarks map to branches.

```bash
# Git (recommended)
git branch -a
git push origin <branch>

# JJ (optional)
jj bookmark list
jj git push --bookmark <name>
```

## File Hygiene

Do not commit generated artifacts:
- `target/`
- `crates/wasm/pkg/`

## Related Docs

- `README.md` - Project overview and quick start
- `docs/INTEGRATION.md` - Mosaic + Triangle.js integration
- `ARCHITECTURE.md` - Full technical breakdown
