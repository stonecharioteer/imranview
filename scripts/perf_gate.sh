#!/usr/bin/env bash
set -euo pipefail

if [[ $# -eq 0 ]]; then
  echo "usage: $0 <log-file> [<log-file> ...]" >&2
  exit 2
fi

failed=0
for log_file in "$@"; do
  if [[ ! -f "$log_file" ]]; then
    echo "[perf-gate][ERROR] missing log file: $log_file" >&2
    failed=1
    continue
  fi

  echo "[perf-gate] scanning $log_file"
  warnings="$(rg -n "\\[WARN\\].*\\(target=.*threshold=.*\\)" "$log_file" || true)"
  if [[ -n "$warnings" ]]; then
    echo "[perf-gate][FAIL] budget warnings found in $log_file"
    echo "$warnings"
    failed=1
  else
    echo "[perf-gate][PASS] no budget warnings in $log_file"
  fi
done

if [[ "$failed" -ne 0 ]]; then
  exit 1
fi

echo "[perf-gate] all logs passed"
