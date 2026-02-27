#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEMPLATES=(rust typescript python go monorepo)

for template in "${TEMPLATES[@]}"; do
  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT

  repo_dir="$tmp_dir/repo"
  mkdir -p "$repo_dir"
  git -C "$repo_dir" init -q
  git -C "$repo_dir" config user.name "Hook Test"
  git -C "$repo_dir" config user.email "hook-test@example.com"

  mock_bin_dir="$tmp_dir/bin"
  mkdir -p "$mock_bin_dir"
  hook_calls_log="$tmp_dir/hook-calls.log"

  cat > "$mock_bin_dir/cruxe" <<'MOCK'
#!/usr/bin/env bash
set -euo pipefail

echo "$*" >> "${HOOK_CALLS_LOG:?HOOK_CALLS_LOG must be set}"

if [ "${CRUXE_MOCK_FAIL:-0}" = "1" ]; then
  exit 99
fi
MOCK
  chmod +x "$mock_bin_dir/cruxe"

  cp "$ROOT_DIR/configs/templates/$template/hooks/post-commit" "$repo_dir/.git/hooks/post-commit"
  cp "$ROOT_DIR/configs/templates/$template/hooks/pre-push" "$repo_dir/.git/hooks/pre-push"
  chmod +x "$repo_dir/.git/hooks/post-commit" "$repo_dir/.git/hooks/pre-push"

  echo "hello" > "$repo_dir/a.txt"
  git -C "$repo_dir" add a.txt
  PATH="$mock_bin_dir:$PATH" HOOK_CALLS_LOG="$hook_calls_log" git -C "$repo_dir" commit -q -m "test commit"

  if ! grep -q '^sync --workspace ' "$hook_calls_log"; then
    echo "[$template] expected sync invocation was not recorded" >&2
    exit 1
  fi

  PATH="$mock_bin_dir:$PATH" HOOK_CALLS_LOG="$hook_calls_log" "$repo_dir/.git/hooks/pre-push"

  if ! grep -q '^doctor --path ' "$hook_calls_log"; then
    echo "[$template] expected doctor invocation was not recorded" >&2
    exit 1
  fi

  # Fail-soft check: mocked failures must not block hooks.
  PATH="$mock_bin_dir:$PATH" HOOK_CALLS_LOG="$hook_calls_log" CRUXE_MOCK_FAIL=1 "$repo_dir/.git/hooks/pre-push"
  PATH="$mock_bin_dir:$PATH" HOOK_CALLS_LOG="$hook_calls_log" CRUXE_MOCK_FAIL=1 "$repo_dir/.git/hooks/post-commit"

  rm -rf "$tmp_dir"
  trap - EXIT
done

echo "auto-indexing hook template tests passed"
