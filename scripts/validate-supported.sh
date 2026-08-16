#!/usr/bin/env bash
# Repeatable validation for FE behavior that is currently implemented. This script deliberately
# does not assert support for deferred gameplay, client-effect, or TFS runtime systems.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT_DIR"

RUST_TOOLCHAIN=${FE_RUST_TOOLCHAIN:-+stable}
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/forgotten-engine-validation.XXXXXX")
WORLD_DIR="$WORK_DIR/world"
SERVER_LOG="$WORK_DIR/server.log"
trap 'rm -rf "$WORK_DIR"' EXIT

run_cargo() {
  cargo "$RUST_TOOLCHAIN" "$@"
}

port_is_bound() {
  netstat -ltn 2>/dev/null | grep -Eq "[.:]$1[[:space:]]"
}

choose_port_base() {
  local candidate offset
  for _ in $(seq 1 100); do
    candidate=$((20000 + RANDOM % 30000))
    for offset in 0 1 2 3 4; do
      if port_is_bound "$((candidate + offset))"; then
        break
      fi
    done
    if [[ "$offset" -eq 4 ]] && ! port_is_bound "$((candidate + offset))"; then
      printf '%s\n' "$candidate"
      return 0
    fi
  done
  echo 'unable to select an unused local validation port range' >&2
  return 1
}

echo '==> formatting'
run_cargo fmt --all -- --check

echo '==> linting'
run_cargo clippy --workspace --all-targets -- -D warnings

echo '==> workspace tests'
run_cargo test --workspace

echo '==> debug build'
run_cargo build --workspace
ENGINE="$ROOT_DIR/target/debug/forgotten-engine"

echo '==> stable CLI smoke checks'
"$ENGINE" version
"$ENGINE" compatibility --json >/dev/null
"$ENGINE" init "$WORLD_DIR" --profile fe-7.4
PORT_BASE=$(choose_port_base)
sed -i -E \
  -e "s/^gameProtocolPort = [0-9]+/gameProtocolPort = ${PORT_BASE}/" \
  -e "s/^statusProtocolPort = [0-9]+/statusProtocolPort = $((PORT_BASE + 1))/" \
  -e "s/^gameSessionPort = [0-9]+/gameSessionPort = $((PORT_BASE + 2))/" \
  -e "s/^otclientV8LoginPort = [0-9]+/otclientV8LoginPort = $((PORT_BASE + 3))/" \
  -e "s/^otclientV8GamePort = [0-9]+/otclientV8GamePort = $((PORT_BASE + 4))/" \
  "$WORLD_DIR/config.lua"
"$ENGINE" validate "$WORLD_DIR"
"$ENGINE" status "$WORLD_DIR"
"$ENGINE" generate-key "$WORLD_DIR"

ACCOUNT_OUTPUT=$("$ENGINE" account create "$WORLD_DIR" 100000 fe-validation-password)
ACCOUNT_ID=$(printf '%s\n' "$ACCOUNT_OUTPUT" | sed -n 's/.*native-account-id=\([0-9][0-9]*\).*/\1/p')
if [[ -z "$ACCOUNT_ID" ]]; then
  echo 'failed to read the local native account ID from provisioning output' >&2
  exit 1
fi
PLAYER_OUTPUT=$("$ENGINE" player create "$WORLD_DIR" "$ACCOUNT_ID" ValidationKnight)
PLAYER_ID=$(printf '%s\n' "$PLAYER_OUTPUT" | sed -n 's/.*player-id=\([0-9][0-9]*\).*/\1/p')
if [[ -z "$PLAYER_ID" ]]; then
  echo 'failed to read the local player ID from provisioning output' >&2
  exit 1
fi
"$ENGINE" player vocation "$WORLD_DIR" "$PLAYER_ID" 4
"$ENGINE" player town "$WORLD_DIR" "$PLAYER_ID" 0
"$ENGINE" player skill "$WORLD_DIR" "$PLAYER_ID" sword 12 25
"$ENGINE" player equip "$WORLD_DIR" "$PLAYER_ID" right 100 1
"$ENGINE" player unequip "$WORLD_DIR" "$PLAYER_ID" right
"$ENGINE" player container-create "$WORLD_DIR" "$PLAYER_ID" 0 100 5 validation
"$ENGINE" player container-add "$WORLD_DIR" "$PLAYER_ID" 0 101 1
"$ENGINE" player container-remove "$WORLD_DIR" "$PLAYER_ID" 0 0
"$ENGINE" command "$WORLD_DIR" broadcast 'validation smoke event'
"$ENGINE" backup "$WORLD_DIR"
"$ENGINE" validate "$WORLD_DIR"

echo '==> bounded host startup smoke check'
set +e
timeout --signal=INT --kill-after=3s 3s "$ENGINE" run "$WORLD_DIR" >"$SERVER_LOG" 2>&1
HOST_STATUS=$?
set -e
if [[ "$HOST_STATUS" -ne 0 && "$HOST_STATUS" -ne 124 ]]; then
  cat "$SERVER_LOG" >&2
  exit "$HOST_STATUS"
fi
grep -q 'Server host online' "$SERVER_LOG"

echo '==> supported automated validation passed'
echo 'Manual unmodified-OTClientV8 checks remain required; see docs/validation-baseline.md.'
