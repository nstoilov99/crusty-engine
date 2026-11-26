#!/usr/bin/env bash
# Local mirror of .github/workflows/ci.yml. Run before calling work done.
#
#   bash scripts/ci_local.sh            full gate (what CI runs)
#   bash scripts/ci_local.sh --quick    check + clippy + fmt, both feature sets
#
# Exits non-zero on the first failing step and names it.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TERM_COLOR=always
# CMake 4 refuses shaderc-sys' bundled glslang sources without this (see ci.yml).
export CMAKE_POLICY_VERSION_MINIMUM="${CMAKE_POLICY_VERSION_MINIMUM:-3.5}"

step() { local name=$1; shift; printf '\n==> %s\n' "$name"; "$@" || { printf 'FAILED: %s\n' "$name"; exit 1; }; }

step "fmt --check"      cargo fmt --all -- --check
step "clippy (default)" cargo clippy --workspace --lib --bins --tests -- -D warnings
step "clippy (editor)"  cargo clippy --workspace --lib --bins --tests --features editor -- -D warnings

if [[ "${1:-}" == "--quick" ]]; then echo; echo "Quick gate passed."; exit 0; fi

step "build (default)"  cargo build --workspace
step "build (editor)"   cargo build -p game_client --features editor
step "test (default)"   cargo test --workspace --lib --tests
step "test (editor)"    cargo test --workspace --lib --tests --features editor

step "graph crates standalone" cargo check -p node_graph_types -p node_graph_exec -p curve_asset
if rustup target list --installed | grep -q '^wasm32-unknown-unknown$'; then
  step "wasm32 game_shared"  cargo check -p game_shared --target wasm32-unknown-unknown
  step "wasm32 graph crates" cargo check -p node_graph_types -p node_graph_exec -p curve_asset --target wasm32-unknown-unknown
else
  echo; echo "(skipping wasm32 checks: run 'rustup target add wasm32-unknown-unknown')"
fi

step "design lint" bash scripts/lint_design.sh

echo; echo "Full gate passed."
