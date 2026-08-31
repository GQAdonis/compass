#!/usr/bin/env bash
set -euo pipefail

QUALIFY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$QUALIFY_ROOT/target}"
case "$CARGO_TARGET_DIR" in
  /*) ;;
  *) CARGO_TARGET_DIR="$QUALIFY_ROOT/$CARGO_TARGET_DIR" ;;
esac
export CARGO_TARGET_DIR

usage() {
  echo "usage: $0 --fixtures-only" >&2
  exit 2
}

[[ "${1:-}" == "--fixtures-only" && "$#" -eq 1 ]] || usage
mkdir -p "$CARGO_TARGET_DIR"
[[ -d "$CARGO_TARGET_DIR" && -w "$CARGO_TARGET_DIR" ]] || {
  echo "[agent-graph] target directory is not writable: $CARGO_TARGET_DIR" >&2
  exit 1
}

cd "$QUALIFY_ROOT"

echo "[agent-graph] validate frozen JSON contracts"
python3 scripts/check_agent_graph_contracts.py

echo "[agent-graph] qualify Grounding, identity, CRUD, conflicts, rebase, limits, corruption, and audit"
cargo test -p compass-agent-graph --locked

echo "[agent-graph] qualify exact Effective Graph query and current/historical orchestration"
cargo test -p compass-query --test effective_graph --locked
cargo test -p compass-core --test agent_graph --locked

echo "[agent-graph] qualify CLI and MCP authorization adapters"
cargo test -p compass-cli --test agent_graph_cli --locked
cargo test -p compass-mcp --test agent_graph_tools --locked
cargo test -p compass-mcp --test agent_graph_http_auth --locked

echo "[agent-graph] fixture qualification passed"
