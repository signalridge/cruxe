#!/usr/bin/env bash
set -euo pipefail

base_ref="${GITHUB_BASE_REF:-}"
if [[ -n "${base_ref}" ]]; then
  git fetch origin "${base_ref}" --depth=1 >/dev/null 2>&1 || true
  changed_files="$(git diff --name-only "origin/${base_ref}...HEAD")"
else
  if git rev-parse HEAD~1 >/dev/null 2>&1; then
    changed_files="$(git diff --name-only HEAD~1..HEAD)"
  else
    changed_files="$(git ls-files)"
  fi
fi

if [[ -z "${changed_files}" ]]; then
  echo "INFO TRACE_GATE: no changed files detected"
  echo "Next: continue"
  exit 0
fi

echo "Changed files:"
echo "${changed_files}"

if ! grep -q '^openspec/' <<<"${changed_files}"; then
  if grep -Eq '^(crates/|specs/|configs/|Cargo\.toml|Cargo\.lock|README\.md)' <<<"${changed_files}"; then
    echo "BLOCK TRACE_GATE: code/spec changes detected without openspec trace artifacts"
    echo "Next: update openspec/changes/... artifacts for this implementation change"
    exit 1
  fi
fi

active_change_files="$(
  git ls-files "openspec/changes/*" \
    | grep -Ev '^openspec/changes/archive/' || true
)"
if [[ -n "${active_change_files}" ]]; then
  active_changes="$(awk -F/ '{print $3}' <<<"${active_change_files}" | sort -u)"
  echo "BLOCK TRACE_GATE: non-archived OpenSpec change artifacts detected"
  echo "Active change directories:"
  echo "${active_changes}"
  echo "Next: archive active openspec/changes/<name>/ artifacts before merge"
  exit 1
fi

echo "INFO TRACE_GATE: openspec trace requirement satisfied"
echo "Next: continue"
