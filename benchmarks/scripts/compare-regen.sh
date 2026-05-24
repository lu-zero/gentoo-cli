#!/usr/bin/env bash
# compare-regen.sh — wall-clock comparison of metadata-cache regeneration
# tools against the same Gentoo tree.
#
# Tools:
#   em regen -j N           — our Rust implementation (portage-cli)
#   pk repo metadata regen  — pkgcraft's implementation (Rust)
#   egencache --update      — Portage's official tool (Python)
#                             [opt-in via INCLUDE_EGENCACHE=1]
#
# em and pk both accept a custom output directory (-o / -p) so they
# write into isolated tempdirs.  egencache only writes into the repo
# itself (metadata/md5-cache), which means running it overwrites the
# real cache for the duration of the test — so it's opt-in via
# INCLUDE_EGENCACHE=1 and the script will save+restore the existing
# cache around it.
#
# Usage: compare-regen.sh [jobs...]   (default: 24)
#   GENTOO_REPO=<path>     repo to regen (default: /var/db/repos/gentoo)
#   EM=<path>              em binary (default: search PATH then portage-bench tree)
#   EGENCACHE=<path>       egencache (default: PATH)
#   PK=<path>              pkgcraft pk binary (default: PATH then in-tree build)
#   ITERATIONS=N           runs per tool (default: 1)
#   SKIP=tool,tool         comma-separated list of tools to skip
#                          (em | egencache | pk)

set -euo pipefail

REPO="${GENTOO_REPO:-/var/db/repos/gentoo}"
ITERATIONS="${ITERATIONS:-1}"
SKIP="${SKIP:-}"
INCLUDE_EGENCACHE="${INCLUDE_EGENCACHE:-0}"

if [[ $# -gt 0 ]]; then JOBS=("$@"); else JOBS=(24); fi

# Locate binaries.
EM="${EM:-}"
if [[ -z "$EM" ]]; then
    if command -v em >/dev/null 2>&1; then
        EM=$(command -v em)
    elif [[ -x "$SCRIPT_DIR/../../target/release/em" ]]; then
        EM=$(realpath "$SCRIPT_DIR/../../target/release/em")
    fi
fi

PK="${PK:-}"
if [[ -z "$PK" ]]; then
    if command -v pk >/dev/null 2>&1; then
        PK=$(command -v pk)
    elif [[ -x ../pkgcraft/target/release/pk ]]; then
        PK=$(realpath ../pkgcraft/target/release/pk)
    fi
fi

EGENCACHE="${EGENCACHE:-$(command -v egencache 2>/dev/null || true)}"

# egencache wants the repo *name*, plus a config snippet pointing at the path.
REPO_NAME=""
if [[ -f "$REPO/profiles/repo_name" ]]; then
    REPO_NAME=$(< "$REPO/profiles/repo_name")
fi

skip_tool() { [[ ",$SKIP," == *",$1,"* ]]; }

declare -a TOOLS=()
declare -A TOOL_DESC=()
if [[ -n "$EM" && -x "$EM" ]] && ! skip_tool em; then
    TOOLS+=(em); TOOL_DESC[em]="$EM"
fi
if [[ "$INCLUDE_EGENCACHE" == "1" && -n "$EGENCACHE" && -n "$REPO_NAME" ]] \
        && ! skip_tool egencache; then
    TOOLS+=(egencache); TOOL_DESC[egencache]="$EGENCACHE (repo=$REPO_NAME, writes to $REPO)"
fi
if [[ -n "$PK" && -x "$PK" ]] && ! skip_tool pk; then
    TOOLS+=(pk); TOOL_DESC[pk]="$PK"
fi

if [[ ${#TOOLS[@]} -eq 0 ]]; then
    echo "no tools found to compare" >&2; exit 1
fi

echo "Repository: $REPO"
echo "Iterations per tool: $ITERATIONS"
echo "Jobs sweep: ${JOBS[*]}"
echo "Tools:"
for t in "${TOOLS[@]}"; do echo "  $t  ->  ${TOOL_DESC[$t]}"; done
echo

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

# Run a tool. $1=tool, $2=jobs, $3=out_dir, $4=log_path.
# Times the actual command with `time`; prints "real user sys" on stdout.
run_one() {
    local tool="$1" jobs="$2" out_dir="$3" log="$4"
    local tf
    tf=$(mktemp)

    case "$tool" in
        em)
            { time "$EM" --repo "$REPO" regen -o "$out_dir" -j "$jobs" \
                >"$log" 2>&1; } 2>"$tf"
            ;;
        egencache)
            # egencache writes to $REPO/metadata/md5-cache.  We save the
            # existing cache to a sibling directory, let egencache rebuild,
            # then restore.  Requires write access to $REPO.
            local cache="$REPO/metadata/md5-cache"
            local backup="$REPO/metadata/md5-cache.backup-$$"
            local existed=0
            if [[ -d "$cache" ]]; then
                mv "$cache" "$backup"
                existed=1
            fi
            mkdir -p "$cache"
            { time "$EGENCACHE" --update --repo "$REPO_NAME" \
                --jobs="$jobs" \
                >"$log" 2>&1; } 2>"$tf" || true
            # Restore.
            rm -rf "$cache"
            if [[ "$existed" == "1" ]]; then
                mv "$backup" "$cache"
            fi
            ;;
        pk)
            { time "$PK" repo metadata regen -j "$jobs" -p "$out_dir" -f -n "$REPO" \
                >"$log" 2>&1; } 2>"$tf"
            ;;
        *)
            echo "unknown tool: $tool" >&2; rm -f "$tf"; return 1
            ;;
    esac

    awk '/real|user|sys/{printf "%s ", $2} END{print ""}' "$tf"
    rm -f "$tf"
}

printf "%-12s  %-4s  %-4s  %-12s  %-12s  %-12s\n" \
    "tool" "j" "run" "real" "user" "sys"

for J in "${JOBS[@]}"; do
    for tool in "${TOOLS[@]}"; do
        for ((i = 1; i <= ITERATIONS; i++)); do
            out_dir="$WORK/$tool-j$J-i$i"
            log="$WORK/$tool-j$J-i$i.log"
            mkdir -p "$out_dir"
            read -r real user sys < <(run_one "$tool" "$J" "$out_dir" "$log")
            printf "%-12s  %-4s  %-4s  %-12s  %-12s  %-12s\n" \
                "$tool" "$J" "$i" "$real" "$user" "$sys"
        done
    done
done
