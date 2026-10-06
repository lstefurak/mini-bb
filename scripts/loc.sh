#!/usr/bin/env bash
# Line-budget gate (spec 001 NF-1): count code lines in src/**/*.rs,
# excluding blank lines and lines whose first non-space chars are `//`.
# Block comments are banned by CLAUDE.md so this stays honest.
# Counting stops at `#[cfg(test)]` (NF-4: tests are unbudgeted), so the
# test module must be the last item in each file.
set -euo pipefail
cd "$(dirname "$0")/.."

# dir:budget pairs — engine (NF-1) and kafka demo (spec 003 NF-6).
BUDGETS="src:500 kafka-demo/src:300"
fail=0
for pair in $BUDGETS; do
  dir=${pair%%:*}; budget=${pair##*:}
  [ -d "$dir" ] || continue
  total=0
  printf '%-24s %s\n' 'file' 'code lines'
  while IFS= read -r f; do
    n=$(awk '/^#\[cfg\(test\)\]/ { exit }
             { line=$0; sub(/^[ \t]+/, "", line) }
             line != "" && line !~ /^\/\// { c++ } END { print c+0 }' "$f")
    printf '%-24s %d\n' "$f" "$n"
    total=$((total + n))
  done < <(find "$dir" -name '*.rs' | sort)
  echo "------------------------"
  echo "$dir total: $total / $budget"
  if [ "$total" -gt "$budget" ]; then
    echo "FAIL: $dir over budget by $((total - budget)) lines" >&2
    fail=1
  fi
  echo
done
[ "$fail" -eq 0 ] && echo "OK" || exit 1
