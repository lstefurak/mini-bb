#!/usr/bin/env bash
# Line-budget gate (spec 001 NF-1): count code lines in src/**/*.rs,
# excluding blank lines and lines whose first non-space chars are `//`.
# Block comments are banned by CLAUDE.md so this stays honest.
# Counting stops at `#[cfg(test)]` (NF-4: tests are unbudgeted), so the
# test module must be the last item in each file.
set -euo pipefail
cd "$(dirname "$0")/.."

BUDGET=500
total=0
printf '%-20s %s\n' 'file' 'code lines'
while IFS= read -r f; do
  n=$(awk '/^#\[cfg\(test\)\]/ { exit }
           { line=$0; sub(/^[ \t]+/, "", line) }
           line != "" && line !~ /^\/\// { c++ } END { print c+0 }' "$f")
  printf '%-20s %d\n' "$f" "$n"
  total=$((total + n))
done < <(find src -name '*.rs' | sort)

echo "--------------------"
echo "total: $total / $BUDGET"
if [ "$total" -gt "$BUDGET" ]; then
  echo "FAIL: over budget by $((total - BUDGET)) lines" >&2
  exit 1
fi
echo "OK"
