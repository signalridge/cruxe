#!/usr/bin/env bash
set -euo pipefail

msg_file="${1:-}"
if [[ -z "${msg_file}" || ! -f "${msg_file}" ]]; then
  echo "BLOCK CONVENTIONAL_COMMIT: commit message file is missing"
  echo "Next: provide a valid commit message file to the commit-msg hook"
  exit 1
fi

msg="$(head -n 1 "${msg_file}")"
re='^(feat|fix|docs|refactor|test|chore|perf|ci|build|style)(\([^)]+\))?: .+$'

if [[ ! "${msg}" =~ ${re} ]]; then
  echo "BLOCK CONVENTIONAL_COMMIT: commit title must match <type>(<scope>): <summary>"
  echo "Next: rewrite the first commit message line using a conventional commit format"
  exit 1
fi

echo "INFO CONVENTIONAL_COMMIT: commit message format is valid"
echo "Next: continue"
