default:
    @just --list

# --all-targets so test and bench targets get type-checked too, --workspace because
# default-members is ["gui"] and a bare cargo invocation only sees that crate.

check:
    cargo check --workspace --all-features --all-targets

clippy:
    cargo clippy --workspace --all-features --all-targets

test:
    cargo test --workspace --all-features

# The web build: only gui and its dependencies, since macros is a host proc-macro crate
wasm:
    cargo build -p gui --target wasm32-unknown-unknown

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

# Everything CI runs
suite:
    @just check
    @just test
    @just wasm
    @just fmt-check
    @just clippy
