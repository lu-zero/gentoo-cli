#!/usr/bin/env bash
# compare-search.sh — wall-clock comparison of package search tools.
#
# Tools:
#   em search <pattern>      — our Rust implementation (reads md5-cache)
#   emerge -s <pattern>      — Portage's search (reads md5-cache, Python startup)
#   qsearch -S <pattern>     — portage-utils C searcher (reads md5-cache, name+desc)
#                             [skipped if not found]
#
# All three tools read from the repo's metadata/md5-cache.
#
# Pkgcraft has no direct equivalent in `pk pkg`, so it is not included.
#
# Usage: compare-search.sh [pattern ...]   (default: gcc python firefox rust)
#   GENTOO_REPO=<path>     repo to search (default: /var/db/repos/gentoo)
#   EM=<path>              em binary (default: PATH then portage-bench tree)
#   EMERGE=<path>          emerge binary (default: PATH)
#   QSEARCH=<path>         qsearch binary (default: PATH; skipped if absent)
#   ITERATIONS=N           runs per (tool, pattern) (default: 3)
#   WARMUP=1               do one untimed warmup per (tool, pattern)
#                          to prime page cache (default: 1, 0 to disable)

set -euo pipefail

REPO="${GENTOO_REPO:-/var/db/repos/gentoo}"
ITERATIONS="${ITERATIONS:-3}"
WARMUP="${WARMUP:-1}"

if [[ $# -gt 0 ]]; then PATTERNS=("$@"); else PATTERNS=(gcc python firefox rust); fi

EM="${EM:-}"
if [[ -z "$EM" ]]; then
    if command -v em >/dev/null 2>&1; then
        EM=$(command -v em)
    elif [[ -x "$SCRIPT_DIR/../../target/release/em" ]]; then
        EM=$(realpath "$SCRIPT_DIR/../../target/release/em")
    fi
fi
if [[ -z "$EM" || ! -x "$EM" ]]; then
    echo "em not found. Set EM=<path> or build portage-cli first." >&2
    exit 1
fi

EMERGE="${EMERGE:-$(command -v emerge 2>/dev/null || true)}"
if [[ -z "$EMERGE" ]]; then
    echo "emerge not found in PATH." >&2
    exit 1
fi

QSEARCH="${QSEARCH:-$(command -v qsearch 2>/dev/null || true)}"

echo "Repository: $REPO"
echo "Patterns:   ${PATTERNS[*]}"
echo "Iterations: $ITERATIONS  (warmup: $WARMUP)"
echo "em:         $EM"
echo "emerge:     $EMERGE"
echo "qsearch:    ${QSEARCH:-<not found, skipped>}"
echo

# real time only (search is short, ms-scale). Capture via /usr/bin/time -f.
time_one() {
    local tf
    tf=$(mktemp)
    /usr/bin/time -f "%e" -o "$tf" "$@" > /dev/null 2>&1 || true
    cat "$tf"
    rm -f "$tf"
}

# Mean of N runs, with optional one warmup that is discarded.
sample_mean() {
    local tool="$1"; shift
    local -a cmd=("$@")
    local total=0 i
    if [[ "$WARMUP" == "1" ]]; then
        time_one "${cmd[@]}" > /dev/null
    fi
    local samples=()
    for ((i = 1; i <= ITERATIONS; i++)); do
        local t
        t=$(time_one "${cmd[@]}")
        samples+=("$t")
        total=$(awk -v a="$total" -v b="$t" 'BEGIN{print a+b}')
    done
    local mean
    mean=$(awk -v t="$total" -v n="$ITERATIONS" 'BEGIN{printf "%.3f", t/n}')
    # also compute min for stability hint
    local min
    min=$(printf '%s\n' "${samples[@]}" | sort -n | head -1)
    printf "mean=%s s   min=%s s" "$mean" "$min"
}

# Header
printf "%-10s  %-15s  %s\n" "tool" "pattern" "timing"

for pat in "${PATTERNS[@]}"; do
    em_result=$(sample_mean em      "$EM" --repo "$REPO" search "$pat")
    emerge_result=$(sample_mean emerge "$EMERGE" -s "$pat")
    printf "%-10s  %-15s  %s\n" "em"     "$pat" "$em_result"
    printf "%-10s  %-15s  %s\n" "emerge" "$pat" "$emerge_result"
    if [[ -n "$QSEARCH" && -x "$QSEARCH" ]]; then
        qsearch_result=$(sample_mean qsearch "$QSEARCH" -S "$pat")
        printf "%-10s  %-15s  %s\n" "qsearch" "$pat" "$qsearch_result"
    fi
done
