#!/usr/bin/env bash
# bench-regen.sh — benchmark regen_only at multiple thread counts
# Usage: bench-regen.sh [jobs...]   (default: 4 8 16 20 24 32 40)
#   GENTOO_REPO=<path>  override repo path
#   DEDUP=1             pass --dedup to regen_only
#   LASSO=1             build with --features lasso (lasso interner)
#   SYMBOL_TABLE=1      build with --features symbol-table (symbol-table interner)
#   MIMALLOC=1          build with --features mimalloc (mimalloc allocator)
#   DHAT=1              build with --features dhat-heap (dhat profiler)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO="${GENTOO_REPO:-/var/db/repos/gentoo}"
DEDUP_FLAG=""
[[ "${DEDUP:-0}" == "1" ]] && DEDUP_FLAG="--dedup"

# Collect feature flags from environment
FEATURES=()
[[ "${LASSO:-0}" == "1" ]] && FEATURES+=("lasso")
[[ "${SYMBOL_TABLE:-0}" == "1" ]] && FEATURES+=("symbol-table")
[[ "${MIMALLOC:-0}" == "1" ]] && FEATURES+=("mimalloc")
[[ "${DHAT:-0}" == "1" ]] && FEATURES+=("dhat-heap")

FEATURE_FLAGS=""
if [[ ${#FEATURES[@]} -gt 0 ]]; then
    FEATURE_FLAGS="--features $(IFS=,; echo "${FEATURES[*]}")"
fi

TARGET_DIR=$(cargo metadata --format-version 1 --no-deps --manifest-path "$SCRIPT_DIR/Cargo.toml" 2>/dev/null \
    | python3 -c "import json,sys; print(json.load(sys.stdin)['target_directory'])" 2>/dev/null \
    || echo "$SCRIPT_DIR/target")
REGEN="$TARGET_DIR/release/examples/regen_only"

# Build description for build message
BUILD_DESC=""
if [[ ${#FEATURES[@]} -gt 0 ]]; then
    BUILD_DESC=" (features: $(IFS=,; echo "${FEATURES[*]}"))"
fi

# Build if binary doesn't exist or features are enabled
if [[ ! -x "$REGEN" ]] || [[ ${#FEATURES[@]} -gt 0 ]]; then
    echo "Building regen_only${BUILD_DESC}..." >&2
    cargo build --release --example regen_only $FEATURE_FLAGS --manifest-path "$SCRIPT_DIR/Cargo.toml"
fi

if [[ $# -gt 0 ]]; then JOBS=("$@"); else JOBS=(4 8 16 20 24 32 40); fi

tree_rss() {
    local pid=$1 total=0 rss child
    rss=$(awk '/VmRSS/{print $2}' /proc/"$pid"/status 2>/dev/null || echo 0)
    total=$((total + rss))
    for child in $(pgrep -P "$pid" 2>/dev/null); do
        rss=$(tree_rss "$child")
        total=$((total + rss))
    done
    echo "$total"
}

peak_rss() {
    local pid=$1 max=0 rss
    while kill -0 "$pid" 2>/dev/null; do
        rss=$(tree_rss "$pid")
        [[ $rss -gt $max ]] && max=$rss
        sleep 0.05
    done
    echo "$max"
}

printf "%-4s  %-10s  %-10s  %-10s  %s\n" "j" "real" "user" "sys" "peak RSS"
for J in "${JOBS[@]}"; do
    OUT=$(mktemp -d)
    { time "$REGEN" $DEDUP_FLAG "$REPO" -o "$OUT" -j "$J" >/dev/null 2>&1; } 2>/tmp/bench_time &
    BGPID=$!
    RSS=$(peak_rss "$BGPID")
    wait "$BGPID"
    REAL=$(awk '/real/{print $2}' /tmp/bench_time)
    USER=$(awk '/user/{print $2}' /tmp/bench_time)
    SYS=$(awk '/sys/{print $2}'  /tmp/bench_time)
    printf "%-4s  %-10s  %-10s  %-10s  %d MB\n" "$J" "$REAL" "$USER" "$SYS" "$((RSS / 1024))"
    rm -rf "$OUT"
done
